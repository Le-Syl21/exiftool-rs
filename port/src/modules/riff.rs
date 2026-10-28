//! Image::ExifTool::RIFF (Image/ExifTool/RIFF.pm), ported sub by sub (see docs/port/PORTING.md).
//!
//! Read RIFF/AVI/WAV/WEBP meta information
//!
//! ProcessLucas walks `$$dataPt` with `m//g` matches whose pos() lives on
//! the caller's scalar: those run as Perl statements (`on_data`). The pos()
//! of its own `$buff` is followed in Rust (`gmatch`).

use crate::interp::eval::ER;
use crate::interp::{Interp, Want};
use crate::modules::random_access::{cell, perl_eval};
use crate::perl::Value;
use crate::pl::{self, arg, cap, n, re, ret};

const MODULE: &str = "Image::ExifTool::RIFF";

/// `Image::ExifTool::NAME(ARGS)` (the functions imported with :DataAccess :Utils).
fn call(it: &mut Interp, name: &str, args: Vec<Value>) -> ER<Value> {
    pl::call(it, &format!("Image::ExifTool::{name}"), args)
}

fn m(it: &mut Interp, et: &Value, name: &str, args: Vec<Value>) -> ER<Value> {
    pl::method(it, et, name, args)
}

fn opt(it: &mut Interp, et: &Value, name: &str) -> ER<Value> {
    m(it, et, "Options", vec![pl::s(name)])
}

fn warn(it: &mut Interp, et: &Value, msg: Value) -> ER<()> {
    m(it, et, "Warn", vec![msg])?;
    Ok(())
}

fn handle(it: &mut Interp, et: &Value, tbl: &Value, tag: &str, val: Value) -> ER<()> {
    m(it, et, "HandleTag", vec![tbl.clone(), pl::s(tag), val])?;
    Ok(())
}

fn cat(parts: &[&Value]) -> Value {
    let mut v = Vec::new();
    for p in parts {
        v.extend_from_slice(&p.to_bytes());
    }
    Value::bytes(v)
}

fn lex_hash(it: &mut Interp, name: &str) -> Value {
    crate::runtime::lex_hash(it, MODULE, name)
}

/// `$raf->Read($buff, $len)`: the count, with `buff` updated.
fn read(it: &mut Interp, raf: &Value, buff: &mut Value, len: Value) -> ER<Value> {
    let mut args = [buff.clone(), len];
    let r = pl::method_mut(it, raf, "Read", &mut args, Want::Scalar)?;
    *buff = args[0].clone();
    Ok(r.into_iter().next().unwrap_or_default())
}

/// A Perl statement on `$dataPt` (the caller's buffer reference) with `$p`, in list context.
fn on_data(it: &mut Interp, code: &str, data_pt: &Value, p: &Value) -> ER<Vec<Value>> {
    let c = cell(data_pt.clone());
    let pc = cell(p.clone());
    perl_eval(it, code, MODULE, &[("dataPt", &c), ("p", &pc)], Want::List)
}

/// `$s =~ /PAT/g` in scalar context on a `my` variable whose pos() is
/// `pos`: the captures (`[0]` is the match) when it matched; pos() is
/// moved to the end of the match, or reset when it failed.
fn gmatch(pat: &[u8], flags: &str, s: &Value, pos: &mut Option<usize>) -> ER<Option<pl::Caps>> {
    let rx = match crate::perl::regex::compile(pat, flags) {
        Ok(r) => r,
        Err(e) => return crate::interp::eval::die(e),
    };
    let b = s.to_bytes();
    let start = pos.unwrap_or(0);
    if start > b.len() {
        *pos = None;
        return Ok(None);
    }
    match rx.match_at(&b, start) {
        Ok(Some(sp)) => {
            let (_, e) = sp[0].unwrap();
            *pos = Some(e);
            Ok(Some(sp.iter().map(|g| g.map(|(x, y)| Value::bytes(&b[x..y]))).collect()))
        }
        _ => {
            *pos = None;
            Ok(None)
        }
    }
}

/// AutoLoad our writer routines when necessary
pub fn autoload(it: &mut Interp, a: Vec<Value>, want: Want) -> ER<Vec<Value>> {
    let name = it.get_scalar("Image::ExifTool::RIFF::AUTOLOAD");
    let mut args = vec![name];
    args.extend(a);
    pl::at(1592);
    it.call_named("Image::ExifTool::DoAutoLoad", args, want)
}

/// Convert RIFF date to EXIF format
pub fn convert_riff_date(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let mut val = arg(&a, 0);
    let part = pl::split_ws(&val);
    let mon;
    let month_num = lex_hash(it, "monthNum");
    if part.len() >= 5 && {
        mon = month_num.get(pl::ucfirst(&pl::lc(&part[1])).to_bytes());
        mon.truthy()
    } {
        // the standard AVI date format (eg. "Mon Mar 10 15:04:43 2003")
        val = pl::sprintf("%.4d:%.2d:%.2d %s", &[part[4].clone(), mon, part[2].clone(), part[3].clone()]);
    } else if let Some(c) = re(r"(\d{4})/\s*(\d+)/\s*(\d+)/?\s+(\d+):\s*(\d+)\s*(P?)", "").m(&val) {
        // but the Casio QV-3EX writes dates like "2001/ 1/27  1:42PM",
        // and the Casio EX-Z30 writes "2005/11/28/ 09:19"... doh!
        let h = pl::add(&cap(&c, 4), &n(if cap(&c, 6).truthy() { 12 } else { 0 }));
        val = pl::sprintf("%.4d:%.2d:%.2d %.2d:%.2d:00", &[cap(&c, 1), cap(&c, 2), cap(&c, 3), h, cap(&c, 5)]);
    } else if let Some(c) = re(r"(\d{4})[-/](\d+)[-/](\d+)\s+(\d+:\d+:\d+)", "").m(&val) {
        // the Konica KD500Z writes "2002-12-16  15:35:01\0\0"
        val = cat(&[&cap(&c, 1), &pl::s(":"), &cap(&c, 2), &pl::s(":"), &cap(&c, 3), &pl::s(" "), &cap(&c, 4)]);
    }
    ret(val)
}

/// Print time
/// Inputs: 0) time in seconds
/// Returns: time string
pub fn convert_timecode(_it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let mut val = arg(&a, 0);
    let mut hr = pl::int(&pl::div(&val, &n(3600))?);
    val = pl::sub(&val, &pl::mul(&hr, &n(3600)));
    let mut min = pl::int(&pl::div(&val, &n(60))?);
    val = pl::sub(&val, &pl::mul(&min, &n(60)));
    let mut ss = pl::sprintf("%05.2f", &[val]);
    if pl::ge(&ss, &n(60)) {
        // handle round-off problems
        ss = pl::s("00.00");
        min = pl::add(&min, &n(1));
        if pl::ge(&min, &n(60)) {
            min = pl::sub(&min, &n(60));
            hr = pl::add(&hr, &n(1));
        }
    }
    ret(pl::sprintf("%d:%.2d:%s", &[hr, min, ss]))
}

/// Calculate duration of RIFF
/// Inputs: 0) ExifTool ref, 1/2) RIFF:FrameRate/Count, 2/3) VideoFrameRate/Count
/// Returns: Duration in seconds or undef
/// Notes: Sums duration of all sub-documents (concatenated AVI files)
pub fn calc_duration(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let et = arg(&a, 0);
    let mut val: Vec<Value> = a.iter().skip(1).cloned().collect();
    let mut total_duration = n(0);
    let mut sub_doc = 0i64;
    let mut key_list: Vec<Value> = Vec::new();
    let v = |val: &Vec<Value>, i: usize| arg(val, i);
    loop {
        // this is annoying.  Apparently (although I couldn't verify this), FrameCount
        // in the RIFF header includes multiple video tracks if they exist (eg. with the
        // FujiFilm REAL 3D AVI's), but the video stream information isn't reliable for
        // some cameras (eg. Olympus FE models), so use the video stream information
        // only if the RIFF header duration is 2 to 3 times longer
        // (also see https://github.com/exiftool/exiftool/issues/371)
        let mut dur1 = Value::Undef;
        if v(&val, 0).truthy() {
            pl::at(1659);
            dur1 = pl::div(&v(&val, 1), &v(&val, 0))?;
        }
        if v(&val, 2).truthy() && v(&val, 3).truthy() {
            pl::at(1661);
            let dur2 = pl::div(&v(&val, 3), &v(&val, 2))?;
            pl::at(1662);
            let rat = pl::div(&dur1, &dur2)?;
            if pl::gt(&rat, &Value::Num(1.9)) && pl::lt(&rat, &Value::Num(3.1)) {
                dur1 = dur2;
            }
        }
        if !dur1.is_undef() {
            total_duration = pl::add(&total_duration, &dur1);
        }
        let more = pl::lt(&n(sub_doc), &et.get("DOC_COUNT"));
        sub_doc += 1;
        if !more {
            break;
        }
        // get tag values for next sub-document
        let tags = ["FrameRate", "FrameCount", "VideoFrameRate", "VideoFrameCount"];
        let raw_value = et.get("VALUE");
        for (i, t) in tags.iter().enumerate() {
            let keys;
            if sub_doc == 1 {
                // generate list of available keys for each tag
                keys = Value::new_array(vec![]);
                if key_list.len() <= i {
                    key_list.resize(i + 1, Value::Undef);
                }
                key_list[i] = keys.clone();
                let mut j = 0;
                loop {
                    let key = if j != 0 { format!("{t} ({j})") } else { t.to_string() };
                    if raw_value.get(key.as_bytes()).is_undef() {
                        break;
                    }
                    keys.push(pl::s(&key));
                    j += 1;
                }
            } else {
                keys = arg(&key_list, i);
            }
            // find key for tag in this sub-document
            let mut grp = format!("Doc{sub_doc}");
            if i < 2 {
                grp.push_str(":RIFF"); // (tags 0 and 1 also in RIFF group)
            }
            let key = m(it, &et, "GroupMatches", vec![pl::s(&grp), keys])?;
            if val.len() <= i {
                val.resize(i + 1, Value::Undef);
            }
            val[i] = if key.truthy() { raw_value.get(key.to_bytes()) } else { Value::Undef };
        }
        if !(!v(&val, 0).is_undef() && !v(&val, 1).is_undef()) {
            break; // (Require'd tags)
        }
    }
    ret(total_duration)
}

/// Process stream data
/// Inputs: 0) ExifTool object ref, 1) dirInfo reference, 2) tag table ref
/// Returns: 1 on success
pub fn process_stream_data(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info, tag_tbl) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    let data_pt = dir_info.get("DataPt");
    let start = dir_info.get("DirStart");
    let size = dir_info.get("DirLen");
    if pl::lt(&size, &n(4)) {
        return ret(n(0));
    }
    if opt(it, &et, "Verbose")?.truthy() {
        m(it, &et, "VerboseDir", vec![dir_info.get("DirName"), n(0), size.clone()])?;
    }
    pl::at(1709);
    let tag = pl::substr(&pl::sderef(&data_pt)?, start.to_i64(), Some(4));
    let mut tag_info = m(it, &et, "GetTagInfo", vec![tag_tbl.clone(), tag.clone()])?;
    if !tag_info.truthy() {
        tag_info = m(it, &et, "GetTagInfo", vec![tag_tbl.clone(), pl::s("unknown")])?;
        if !tag_info.truthy() {
            return ret(n(1));
        }
    }
    let subdir = tag_info.get("SubDirectory");
    if tag_info.get("SubDirectory").truthy() {
        let offset = {
            let o = subdir.get("Start");
            if o.truthy() { o } else { n(0) }
        };
        let base_shift = pl::add(&pl::add(&dir_info.get("DataPos"), &dir_info.get("DirStart")), &offset);
        let subdir_info = Value::new_hash();
        subdir_info.set("DataPt", data_pt.clone());
        subdir_info.set("DataPos", pl::sub(&dir_info.get("DataPos"), &base_shift));
        let b = dir_info.get("Base");
        subdir_info.set("Base", pl::add(&if b.truthy() { b } else { n(0) }, &base_shift));
        subdir_info.set("DataLen", dir_info.get("DataLen"));
        subdir_info.set("DirStart", pl::add(&dir_info.get("DirStart"), &offset));
        subdir_info.set("DirLen", pl::sub(&dir_info.get("DirLen"), &offset));
        subdir_info.set("DirName", subdir.get("DirName"));
        subdir_info.set("Parent", dir_info.get("DirName"));
        if !offset.truthy() {
            // allow processing of 2nd directory at the same address
            let addr = pl::add(&pl::add(&subdir_info.get("DirStart"), &subdir_info.get("DataPos")), &subdir_info.get("Base"));
            et.get_or_new_hash("PROCESSED").delete(addr.to_bytes());
        }
        // (we could set FIRST_EXIF_POS to $subdirInfo{Base} here to make
        //  htmlDump offsets relative to EXIF base if we wanted...)
        let sub_table = call(it, "GetTagTable", vec![subdir.get("TagTable")])?;
        m(it, &et, "ProcessDirectory", vec![subdir_info, sub_table])?;
    } else {
        m(
            it,
            &et,
            "HandleTag",
            vec![tag_tbl.clone(), tag, Value::Undef, pl::s("DataPt"), data_pt.clone(), pl::s("DataPos"), dir_info.get("DataPos"), pl::s("Start"), start, pl::s("Size"), size, pl::s("TagInfo"), tag_info],
        )?;
    }
    ret(n(1))
}

/// Make tag information hash for unknown tag
/// Inputs: 0) Tag table ref, 1) tag ID
pub fn make_tag_info(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (tag_tbl, tag) = (arg(&a, 0), arg(&a, 1));
    let mut name = tag.clone();
    let nn = re(r"([\x00-\x1f\x7f-\xff])", "").subst(&mut name, true, |c| {
        let mut v = b"x".to_vec();
        v.extend_from_slice(format!("{:02x}", cap(c, 1).to_bytes()[0]).as_bytes());
        v
    });
    // print in hex if tag is numerical
    if nn > 2 {
        name = pl::sprintf("0x%.4x", &[pl::unpack1("N", &tag)?]);
    }
    let ti = Value::new_hash();
    ti.set("Name", pl::concat(&pl::s("Unknown_"), &name));
    ti.set("Description", pl::concat(&pl::s("Unknown "), &name));
    ti.set("Unknown", n(1));
    ti.set("Binary", n(1));
    ret(call(it, "AddTagToTable", vec![tag_tbl, tag, ti])?)
}

/// Process RIFF chunks
/// Inputs: 0) ExifTool ref, 1) dirInfo ref, 2) tag table ref
/// Returns: 1 on success
pub fn process_chunks(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info, tag_tbl) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    let data_pt = dir_info.get("DataPt");
    let mut start = dir_info.get("DirStart");
    let size = dir_info.get("DirLen");
    let end = pl::add(&start, &size);
    let base = {
        let b = dir_info.get("Base");
        if b.truthy() { b } else { n(0) }
    };
    let verbose = opt(it, &et, "Verbose")?;
    let unknown = opt(it, &et, "Unknown")?;
    let mut charset = opt(it, &et, "CharsetRIFF")?;

    if !charset.truthy() {
        if et.get("CodePage").truthy() {
            charset = et.get("CodePage");
        } else if !charset.is_undef() && charset.eq_str("0") {
            charset = pl::s("Latin");
        }
    }

    if verbose.truthy() {
        m(it, &et, "VerboseDir", vec![dir_info.get("DirName"), n(0), size.clone()])?;
    }

    while pl::lt(&pl::add(&start, &n(8)), &end) {
        pl::at(1795);
        let data = pl::sderef(&data_pt)?;
        let mut tag = pl::substr(&data, start.to_i64(), Some(4));
        let mut len = call(it, "Get32u", vec![data_pt.clone(), pl::add(&start, &n(4))])?;
        start = pl::add(&start, &n(8));
        if pl::gt(&pl::add(&start, &len), &end) {
            warn(it, &et, cat(&[&pl::s("Bad "), &tag, &pl::s(" chunk")]))?;
            return ret(n(0));
        }
        if tag.eq_str("LIST") && pl::ge(&len, &n(4)) {
            tag = cat(&[&tag, &pl::s("_"), &pl::substr(&data, start.to_i64(), Some(4))]);
            len = pl::sub(&len, &n(4));
            start = pl::add(&start, &n(4));
        }
        let tag_info = m(it, &et, "GetTagInfo", vec![tag_tbl.clone(), tag.clone()])?;
        let mut base_shift = n(0);
        let mut val = Value::Undef;
        if tag_info.truthy() {
            if tag_info.get("SubDirectory").truthy() {
                // adjust base if necessary (needed for Ricoh maker notes)
                let mut new_base = tag_info.get("SubDirectory").get("Base");
                if !new_base.is_undef() {
                    // different than your average Base eval...
                    // here we use an absolute $start address
                    start = pl::add(&start, &base);
                    //### eval Base ($start)
                    let scope = crate::runtime::file_scope(it, MODULE);
                    let (v, after) = pl::eval_site_in(it, &new_base, MODULE, Some(scope), &[("start", start.clone())], Want::Scalar);
                    new_base = v;
                    start = arg(&after, 0);
                    base_shift = pl::sub(&new_base, &base);
                    start = pl::sub(&start, &base);
                }
            } else if !tag_info.get("Binary").truthy() {
                let mut format = tag_info.get("Format");
                if !format.truthy() {
                    format = tag_tbl.get("FORMAT");
                }
                if format.truthy() && format.eq_str("string") {
                    val = pl::substr(&data, start.to_i64(), Some(len.to_i64()));
                    re(r"\0+$", "").subst(&mut val, false, |_| Vec::new()); // remove trailing nulls from strings
                    // decode if necessary
                    if charset.truthy() {
                        val = m(it, &et, "Decode", vec![val, charset.clone()])?;
                    }
                }
            }
        } else if verbose.truthy() || unknown.truthy() {
            make_tag_info(it, vec![tag_tbl.clone(), tag.clone()], Want::Void)?;
        }
        let b = pl::add(&base, &base_shift);
        m(
            it,
            &et,
            "HandleTag",
            vec![
                tag_tbl.clone(),
                tag,
                val,
                pl::s("DataPt"),
                data_pt.clone(),
                pl::s("DataPos"),
                pl::sub(&dir_info.get("DataPos"), &base_shift),
                pl::s("Start"),
                start.clone(),
                pl::s("Size"),
                len.clone(),
                pl::s("Base"),
                b.clone(),
                pl::s("Addr"),
                pl::add(&b, &start),
            ],
        )?;
        if len.to_i64() & 0x01 != 0 {
            len = pl::add(&len, &n(1)); // must account for padding if odd number of bytes
        }
        start = pl::add(&start, &len);
    }
    ret(n(1))
}

/// Process BikeBro SGLT chunk (accelerometer data) (ref PH)
/// Inputs: 0) ExifTool ref, 1) dirInfo ref, 2) tag table ref
/// Returns: 1 on success
pub fn process_sglt(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info, tag_tbl) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    let data_pt = dir_info.get("DataPt");
    pl::at(1857);
    let data_len = pl::length(&pl::sderef(&data_pt)?).to_i64();
    let ee = opt(it, &et, "ExtractEmbedded")?;
    // example accelerometer record:
    // 0           1  2  3           4  5           6  7
    // 00 00 00 24 02 00 00 01 17 04 00 00 00 00 00 00 00 00 9b 02
    // frame------ ?? Xs X---------- Ys Y---------- Zs Z----------
    et.set("SET_GROUP1", pl::s("RIFF"));
    et.set("SET_GROUP0", pl::s("RIFF"));
    let mut pos = 0;
    while pos <= data_len - 20 {
        let dc = pl::add(&et.get("DOC_COUNT"), &n(1));
        et.set("DOC_COUNT", dc.clone());
        et.set("DOC_NUM", dc);
        let buff = pl::substr(&data_pt.deref(), pos, None);
        let av = pl::unpack("NCCNCNCN", &buff)?;
        let s = |i: usize| if arg(&av, i).truthy() { n(-1) } else { n(1) };
        let acc = [
            pl::div(&pl::mul(&arg(&av, 3), &s(2)), &Value::Num(1e5))?,
            pl::div(&pl::mul(&arg(&av, 5), &s(4)), &Value::Num(1e5))?,
            pl::div(&pl::mul(&arg(&av, 7), &s(6)), &Value::Num(1e5))?,
        ];
        handle(it, &et, &tag_tbl, "FrameNumber", arg(&av, 0))?;
        handle(it, &et, &tag_tbl, "Accelerometer", pl::join(" ", &acc))?;
        if !ee.truthy() {
            m(it, &et, "Warn", vec![pl::s("Use ExtractEmbedded option to extract all accelerometer data"), n(3)])?;
            break;
        }
        pos += 20;
    }
    et.delete("SET_GROUP0");
    et.delete("SET_GROUP1");
    et.set("DOC_NUM", n(0));
    ret(n(0))
}

/// Process BikeBro SLLT chunk (GPS information) (ref PH)
/// Inputs: 0) ExifTool ref, 1) dirInfo ref, 2) tag table ref
/// Returns: 1 on success
pub fn process_sllt(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info, tag_tbl) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    let data_pt = dir_info.get("DataPt");
    pl::at(1891);
    let data_len = pl::length(&pl::sderef(&data_pt)?).to_i64();
    let ee = opt(it, &et, "ExtractEmbedded")?;
    // example GPS record:
    // 0           1  2     3           4     5           6     7     8  9  10 11    12 13 14 15
    // 00 00 00 17 01 00 00 03 fa 21 ec 00 35 01 6e c0 06 00 08 00 62 10 0b 1b 07 e2 03 0e 57 4e
    // frame------ ?? lonDD lonDDDDDDDD latDD latDDDDDDDD alt-- spd-- hr mn sc yr--- mn dy EW NS
    et.set("SET_GROUP1", pl::s("RIFF"));
    et.set("SET_GROUP0", pl::s("RIFF"));
    let mut pos = 0;
    while pos <= data_len - 30 {
        let dc = pl::add(&et.get("DOC_COUNT"), &n(1));
        et.set("DOC_COUNT", dc.clone());
        et.set("DOC_NUM", dc);
        let buff = pl::substr(&data_pt.deref(), pos, None);
        let av = pl::unpack("NCnNnNnnCCCnCCaa", &buff)?;
        let g = |i: usize| arg(&av, i);
        // - is $a[1] perhaps GPSStatus? (only seen 1, or perhaps record type 1=GPS, 2=acc?)
        let time = pl::sprintf("%.4d:%.2d:%.2d %.2d:%.2d:%.2dZ", &[g(11), g(12), g(13), g(8), g(9), g(10)]);
        handle(it, &et, &tag_tbl, "FrameNumber", g(0))?;
        handle(it, &et, &tag_tbl, "GPSDateTime", time)?;
        let lat = pl::mul(&pl::add(&g(4), &pl::div(&g(5), &Value::Num(1e8))?), &n(if g(15).eq_str("S") { -1 } else { 1 }));
        handle(it, &et, &tag_tbl, "GPSLatitude", lat)?;
        let lon = pl::mul(&pl::add(&g(2), &pl::div(&g(3), &Value::Num(1e8))?), &n(if g(14).eq_str("W") { -1 } else { 1 }));
        handle(it, &et, &tag_tbl, "GPSLongitude", lon)?;
        handle(it, &et, &tag_tbl, "GPSAltitude", g(6))?;
        handle(it, &et, &tag_tbl, "GPSSpeed", g(7))?;
        handle(it, &et, &tag_tbl, "GPSSpeedRef", pl::s("K"))?;
        if !ee.truthy() {
            m(it, &et, "Warn", vec![pl::s("Use ExtractEmbedded option to extract timed GPS"), n(3)])?;
            break;
        }
        pos += 30;
    }
    et.delete("SET_GROUP0");
    et.delete("SET_GROUP1");
    et.set("DOC_NUM", n(0));
    ret(n(1))
}

/// Process Lucas streaming GPS information (Lucas LK-7900 Ace) (ref PH)
/// Inputs: 0) ExifTool ref, 1) dirInfo ref, 2) tag table ref
/// Returns: 1 on success
#[allow(unused_assignments)] // (the `my` list of the Perl, some only written)
pub fn process_lucas(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info, tag_tbl) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    let data_pt = dir_info.get("DataPt");
    pl::at(1931);
    let data_len = pl::length(&pl::sderef(&data_pt)?).to_i64();

    if !opt(it, &et, "ExtractEmbedded")?.truthy() {
        m(it, &et, "Warn", vec![pl::s("Use ExtractEmbedded option to extract timed GPS"), n(3)])?;
        return ret(n(1));
    }
    // record lengths (not including 4-byte ID)
    let rec_len = |rec: &Value| -> i64 {
        match &*rec.to_bytes() {
            b"0GDA" => 24,
            b"0GPS" => 48,
            _ => 0,
        }
    };
    let (mut date, mut time, mut lat, mut lon, mut alt, mut spd, mut sat, mut dop, mut ew, mut ns) =
        (Value::Undef, Value::Undef, Value::Undef, Value::Undef, Value::Undef, Value::Undef, Value::Undef, Value::Undef, Value::Undef, Value::Undef);
    et.set("SET_GROUP1", pl::s("RIFF"));
    et.set("SET_GROUP0", pl::s("RIFF"));
    loop {
        let r = on_data(it, "$$dataPt =~ /(0GDA|0GPS)/g ? ($1, pos($$dataPt)) : ()", &data_pt, &Value::Undef)?;
        if r.is_empty() {
            break;
        }
        let (rec, mut pos) = (arg(&r, 0), arg(&r, 1).to_i64());
        if pos + rec_len(&rec) > data_len {
            warn(it, &et, cat(&[&pl::s("Truncated "), &rec, &pl::s(" record")]))?;
            break;
        }
        let dc = pl::add(&et.get("DOC_COUNT"), &n(1));
        et.set("DOC_COUNT", dc.clone());
        et.set("DOC_NUM", dc);
        // records start with int64u sample date/time in ms since 1970
        let t = pl::div(&call(it, "Get64u", vec![data_pt.clone(), n(pos)])?, &n(1000))?;
        handle(it, &et, &tag_tbl, "SampleDateTime", t)?;
        if rec.eq_str("0GPS") {
            let len = call(it, "Get32u", vec![data_pt.clone(), n(pos + 8)])?.to_i64();
            let end_pos = pos + rec_len(&rec) + len;
            if end_pos > data_len {
                warn(it, &et, pl::s("Truncated 0GPS record"))?;
                break;
            }
            let buff = pl::substr(&data_pt.deref(), pos + rec_len(&rec), Some(len));
            let mut bpos: Option<usize> = None; // pos($buff)
            while let Some(c) = gmatch(br"\$(GC|GA),(\d+),", "", &buff, &mut bpos)? {
                let p = bpos;
                time = cap(&c, 2);
                if cap(&c, 1).eq_str("GC") {
                    //     time   date   dist    ? sat dop alt  A
                    // $GC,052350,180914,0000955,1,08,1.1,0017,,A*45\x0d\x0a\0
                    if let Some(c) = gmatch(br"\G(\d+),\d*,\d*,(\d+),([-\d.]+),(\d+),\d*,A", "", &buff, &mut bpos)? {
                        (date, sat, dop, alt) = (cap(&c, 1), cap(&c, 2), cap(&c, 3), cap(&c, 4));
                    }
                } else {
                    //     time   A lat       lon        spd N W
                    // $GA,052351,A,0949.6626,07635.4439,049,N,E,*4C\x0d\x0a\0
                    if let Some(c) = gmatch(br"\GA,([\d.]+),([\d.]+),(\d+),([NS]),([EW])", "", &buff, &mut bpos)? {
                        (lat, lon, spd, ns, ew) = (cap(&c, 1), cap(&c, 2), cap(&c, 3), cap(&c, 4), cap(&c, 5));
                        // lat/long are in DDDMM.MMMM format
                        let mut deg = pl::int(&pl::div(&lat, &n(100))?);
                        lat = pl::add(&deg, &pl::div(&pl::sub(&lat, &pl::mul(&deg, &n(100))), &n(60))?);
                        deg = pl::int(&pl::div(&lon, &n(100))?);
                        lon = pl::add(&deg, &pl::div(&pl::sub(&lon, &pl::mul(&deg, &n(100))), &n(60))?);
                        if ns.eq_str("S") {
                            lat = pl::mul(&lat, &n(-1));
                        }
                        if ew.eq_str("W") {
                            lon = pl::mul(&lon, &n(-1));
                        }
                    }
                }
                // look ahead to next NMEA-like sentence, and store the fix
                // now only if the next sentence is not at the same time
                let mut pat = br"\$(GC|GA),".to_vec();
                pat.extend_from_slice(&time.to_bytes());
                pat.push(b',');
                if gmatch(&pat, "", &buff, &mut bpos)?.is_none() {
                    on_data(it, "pos($$dataPt) = $p", &data_pt, &n(end_pos))?;
                    let store = match re(r"\$(GC|GA),(\d+)", "").m(&data_pt.deref()) {
                        None => true,
                        Some(c2) => cap(&c2, 1).to_bytes() != time.to_bytes(),
                    };
                    if store {
                        re(r"(\d{2})(\d{2})(\d{2})", "").subst(&mut time, false, |c| cat(&[&cap(c, 1), &pl::s(":"), &cap(c, 2), &pl::s(":"), &cap(c, 3), &pl::s("Z")]).to_bytes().to_vec());
                        if date.truthy() {
                            re(r"(\d{2})(\d{2})(\d{2})", "").subst(&mut date, false, |c| cat(&[&pl::s("20"), &cap(c, 3), &pl::s(":"), &cap(c, 2), &pl::s(":"), &cap(c, 1)]).to_bytes().to_vec());
                            handle(it, &et, &tag_tbl, "GPSDateTime", cat(&[&date, &pl::s(" "), &time]))?;
                        } else {
                            handle(it, &et, &tag_tbl, "GPSTimeStamp", time.clone())?;
                        }
                        if !lat.is_undef() {
                            handle(it, &et, &tag_tbl, "GPSLatitude", lat.clone())?;
                            handle(it, &et, &tag_tbl, "GPSLongitude", lon.clone())?;
                            handle(it, &et, &tag_tbl, "GPSSpeed", spd.clone())?;
                        }
                        if !alt.is_undef() {
                            handle(it, &et, &tag_tbl, "GPSAltitude", alt.clone())?;
                            handle(it, &et, &tag_tbl, "GPSSatellites", sat.clone())?;
                            handle(it, &et, &tag_tbl, "GPSDOP", dop.clone())?;
                        }
                        lat = Value::Undef;
                        alt = Value::Undef;
                    }
                }
                bpos = p;
            }
            pos += len;
        } else {
            // this is an accelerometer (0GDA) record
            // record has 4 more int32s values (the last is always 57 or 58 --
            // maybe related to sample time in ms? -- not extracted)
            let mut acc = pl::unpack(&format!("x{}V3", pos + 8), &data_pt.deref())?;
            // change to signed integer and divide by 256
            for x in acc.iter_mut() {
                if pl::ge(x, &n(0x80000000)) {
                    *x = pl::sub(x, &n(4294967296));
                }
                *x = pl::div(x, &n(256))?;
            }
            handle(it, &et, &tag_tbl, "Accelerometer", pl::join(" ", &acc))?;
        }
        on_data(it, "pos($$dataPt) = $p", &data_pt, &n(pos + rec_len(&rec)))?;
    }
    et.delete("SET_GROUP0");
    et.delete("SET_GROUP1");
    et.set("DOC_NUM", n(0));
    ret(n(1))
}

/// Extract information from a RIFF file
/// Inputs: 0) ExifTool object reference, 1) DirInfo reference
/// Returns: 1 on success, 0 if this wasn't a valid RIFF file
pub fn process_riff(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info) = (arg(&a, 0), arg(&a, 1));
    let raf = dir_info.get("RAF");
    let (mut buff, mut buf2, mut mime, mut err) = (Value::Undef, Value::Undef, Value::Undef, false);
    let (mut rf64, mut movi_end) = (false, Value::Undef);
    let verbose = opt(it, &et, "Verbose")?;
    let unknown = opt(it, &et, "Unknown")?;
    let validate = opt(it, &et, "Validate")?;
    let ee = opt(it, &et, "ExtractEmbedded")?;
    let hash = et.get("ImageDataHash");
    let mut base = n(0);
    let riff_type = lex_hash(it, "riffType");
    let is_image_data = lex_hash(it, "isImageData");
    let type_;

    // verify this is a valid RIFF file
    pl::at(2039);
    if !pl::num_eq(&read(it, &raf, &mut buff, n(12))?, &n(12)) {
        return ret(n(0));
    }
    if let Some(c) = re(r"^(RIFF|RF64)....(.{4})", "s").m(&buff) {
        type_ = riff_type.get(cap(&c, 2).to_bytes());
        if cap(&c, 1).eq_str("RF64") {
            rf64 = true;
        }
    } else {
        // minimal support for a few obscure lossless audio formats...
        let Some(c) = re(r"^(LA0[234]|OFR |LPAC|wvpk)", "").m(&buff) else { return ret(n(0)) };
        if !read(it, &raf, &mut buf2, n(1024))?.truthy() {
            return ret(n(0));
        }
        type_ = riff_type.get(cap(&c, 1).to_bytes());
        buff = pl::concat(&buff, &buf2);
        let mut bpos: Option<usize> = None;
        if gmatch(br"WAVE(.{4})?(junk|fmt )", "s", &buff, &mut bpos)?.is_none() {
            return ret(n(0));
        }
        let p = bpos.unwrap_or(0) as i64;
        if !m(it, &raf, "Seek", vec![n(p - 4), n(0)])?.truthy() {
            return ret(n(0));
        }
        base = n(p - 16);
    }
    if opt(it, &et, "FastScan")?.truthy() {
        raf.set("NoBuffer", n(1)); // disable buffering in FastScan mode
    }
    if type_.truthy() {
        mime = lex_hash(it, "riffMimeType").get(type_.to_bytes());
    }
    m(it, &et, "SetFileType", vec![type_.clone(), mime])?;
    let values = et.get("VALUE");
    if rf64 && values.get("FileType").truthy() {
        values.set("FileType", pl::concat(&values.get("FileType"), &pl::s(" (RF64)")));
    }
    et.set("RIFFStreamType", pl::s("")); // initialize stream type
    et.set("RIFFStreamCodec", Value::new_array(vec![])); // initialize codec array
    call(it, "SetByteOrder", vec![pl::s("II")])?;
    let mut riff_end = pl::add(&call(it, "Get32u", vec![pl::sref(buff.clone()), n(4)])?, &n(8));
    riff_end = pl::add(&riff_end, &n(riff_end.to_i64() & 0x01)); // (account for padding)
    let tag_tbl = call(it, "GetTagTable", vec![pl::s("Image::ExifTool::RIFF::Main")])?;
    let mut pos = n(12);
    let large_file_support = |it: &mut Interp, et: &Value| opt(it, et, "LargeFileSupport");
    //
    // Read chunks in RIFF image
    //
    loop {
        if err {
            if !movi_end.truthy() {
                break;
            }
            // we arrived here because there was a problem parsing the movie data
            // so seek to the end to continue processing
            if pl::gt(&movi_end, &n(0x7fffffff)) {
                if !large_file_support(it, &et)?.truthy() {
                    warn(it, &et, pl::s("Possibly corrupt LIST_movi data"))?;
                    warn(it, &et, pl::s("Stopped parsing at large LIST_movi chunk (LargeFileSupport not set)"))?;
                    err = false;
                    break;
                }
                if large_file_support(it, &et)?.eq_str("2") {
                    warn(it, &et, pl::s("Processing large chunk (LargeFileSupport is 2)"))?;
                }
            }
            if validate.truthy() {
                // (must actually try to read something after seeking to detect error)
                let ok = m(it, &raf, "Seek", vec![pl::sub(&movi_end, &n(1)), n(0)])?.truthy() && pl::num_eq(&read(it, &raf, &mut buff, n(1))?, &n(1));
                if !ok {
                    break;
                }
            } else if !m(it, &raf, "Seek", vec![movi_end.clone(), n(0)])?.truthy() {
                break;
            }
            pos = movi_end.clone();
            warn(it, &et, pl::s("Possibly corrupt LIST_movi data"))?;
            err = false;
            movi_end = Value::Undef;
        }
        if movi_end.truthy() {
            if pl::gt(&pos, &movi_end) {
                err = true; // error if we parsed past the end of the movie data
                continue;
            }
            if pl::num_eq(&pos, &movi_end) {
                movi_end = Value::Undef; // parsed all movie data?
            }
        }
        let num = read(it, &raf, &mut buff, n(8))?;
        if pl::lt(&num, &n(8)) {
            if movi_end.truthy() {
                err = true;
                continue;
            }
            if num.truthy() {
                err = true;
            }
            if validate.truthy() && !pl::num_eq(&pos, &riff_end) {
                warn(it, &et, cat(&[&pl::s("Incorrect RIFF chunk size"), &pl::s(" "), &pos, &pl::s(" vs. "), &riff_end]))?;
            }
            break;
        }
        pos = pl::add(&pos, &n(8));
        let u = pl::unpack("a4V", &buff)?;
        let (mut tag, mut len) = (arg(&u, 0), arg(&u, 1));
        // tweak WEBP type if this is an extended WebP
        if tag.eq_str("VP8X") && type_.eq_str("WEBP") {
            m(it, &et, "OverrideFileType", vec![pl::s("Extended WEBP"), Value::Undef, pl::s("webp")])?;
        }
        // special case: construct new tag name from specific LIST type
        if tag.eq_str("LIST") {
            if !pl::num_eq(&read(it, &raf, &mut buff, n(4))?, &n(4)) {
                err = true;
                continue;
            }
            pos = pl::add(&pos, &n(4));
            tag = cat(&[&tag, &pl::s("_"), &buff]);
            len = pl::sub(&len, &n(4)); // already read 4 bytes (the LIST type)
        } else if tag.eq_str("data") {
            if pl::num_eq(&len, &n(0xffffffff)) && et.get("DataSize64").truthy() {
                len = et.get("DataSize64");
            }
            let dl = et.get("RIFFDataLen");
            et.set("RIFFDataLen", pl::add(&if dl.truthy() { dl } else { n(0) }, &len));
        }
        m(it, &et, "VPrint", vec![n(0), cat(&[&pl::s("RIFF '"), &tag, &pl::s("' chunk ("), &len, &pl::s(" bytes of data):\n")])])?;
        if pl::le(&len, &n(0)) {
            if movi_end.truthy() {
                err = true;
                continue;
            }
            if pl::lt(&len, &n(0)) {
                warn(it, &et, pl::s("Invalid chunk length"))?;
            } else if tag.eq_str("\0\0\0\0") {
                // avoid reading through corupted files filled with nulls because it takes forever
                warn(it, &et, pl::s("Encountered empty null chunk. Processing aborted"))?;
            } else {
                continue;
            }
            break;
        }
        // stop when we hit the audio data or AVI index or AVI movie data
        // --> no more because Adobe Bridge stores XMP after this!!
        // (so now we only do this on the FastScan option)
        if opt(it, &et, "FastScan")?.truthy() && (tag.eq_str("data") || tag.eq_str("idx1") || (tag.eq_str("LIST_movi") && !ee.truthy())) {
            m(it, &et, "VPrint", vec![n(0), pl::s("(end of parsing)\n")])?;
            break;
        }
        // RIFF chunks are padded to an even number of bytes
        let len2 = pl::add(&len, &n(len.to_i64() & 0x01));
        // change name of stream txts data depending on the Codec
        if ee.truthy() {
            if let Some(c) = re(r"^(\d{2})tx$", "").m(&tag) {
                let codec = et.get("RIFFStreamCodec").at(cap(&c, 1).to_i64());
                tag = pl::concat(&pl::s("tx_"), &if codec.truthy() { codec } else { pl::s("Unknown") });
                if tag_tbl.get(tag.to_bytes()).is_undef() {
                    tag = pl::s("tx_Unknown");
                }
                let dc = pl::add(&et.get("DOC_COUNT"), &n(1));
                et.set("DOC_COUNT", dc.clone());
                et.set("DOC_NUM", dc);
            }
        }
        let tag_info = tag_tbl.get(tag.to_bytes());
        // (in LIST_movi chunk: ##db = uncompressed DIB, ##dc = compressed DIB, ##wb = audio data)
        if tag_info.truthy() || ((verbose.truthy() || unknown.truthy()) && !re(r"^(data|idx1|LIST_movi|RIFF|\d{2}(db|dc|wb))$", "").is_match(&tag)) {
            if !pl::ge(&read(it, &raf, &mut buff, len2.clone())?, &len) {
                err = true;
                continue;
            }
            if !pl::num_eq(&pl::length(&buff), &len2) {
                warn(it, &et, cat(&[&pl::s("No padding on odd-sized "), &tag, &pl::s(" chunk")]))?;
            }
            if hash.truthy() && is_image_data.get(tag.to_bytes()).truthy() {
                m(it, &hash, "add", vec![buff.clone()])?;
                m(it, &et, "VPrint", vec![n(0), cat(&[&et.get("INDENT"), &pl::s("(ImageDataHash: '"), &tag, &pl::s("' chunk, "), &len2, &pl::s(" bytes)\n")])])?;
            }
            let mut set_groups = Value::Undef;
            if tag_info.truthy() && tag_info.is_hash() && tag_info.get("SetGroups").truthy() {
                set_groups = tag_info.get("SetGroups");
                et.set("SET_GROUP1", set_groups.clone());
                et.set("SET_GROUP0", set_groups.clone());
            }
            if !tag_info.truthy() && (verbose.truthy() || unknown.truthy()) {
                make_tag_info(it, vec![tag_tbl.clone(), tag.clone()], Want::Void)?;
            }
            m(
                it,
                &et,
                "HandleTag",
                vec![
                    tag_tbl.clone(),
                    tag.clone(),
                    buff.clone(),
                    pl::s("DataPt"),
                    pl::sref(buff.clone()),
                    pl::s("DataPos"),
                    n(0), // (relative to Base)
                    pl::s("Start"),
                    n(0),
                    pl::s("Size"),
                    len.clone(),
                    pl::s("Base"),
                    pl::add(&pos, &base),
                ],
            )?;
            if set_groups.truthy() {
                et.delete("SET_GROUP0");
                et.delete("SET_GROUP1");
            }
            if ee.truthy() {
                et.delete("DOC_NUM");
            }
        } else if tag.eq_str("RIFF") {
            if validate.truthy() && !pl::num_eq(&pl::sub(&pos, &n(8)), &riff_end) {
                warn(it, &et, pl::s("Incorrect RIFF chunk size"))?;
            }
            riff_end = pl::add(&riff_end, &pl::add(&len2, &n(8)));
            // don't read into RIFF chunk (eg. concatenated video file)
            if !pl::num_eq(&read(it, &raf, &mut buff, n(4))?, &n(4)) {
                err = true; // (skip RIFF type word)
                continue;
            }
            pos = pl::add(&pos, &n(4));
            // extract information from remaining file as an embedded file
            let dc = pl::add(&et.get("DOC_COUNT"), &n(1));
            et.set("DOC_COUNT", dc.clone());
            et.set("DOC_NUM", dc);
            continue; // (must not increment $pos)
        } else {
            let mut rewind = Value::Undef;
            // do hash if required
            if hash.truthy() && is_image_data.get(tag.to_bytes()).truthy() {
                rewind = m(it, &raf, "Tell", vec![])?;
                m(it, &et, "ImageDataHash", vec![raf.clone(), len2.clone(), cat(&[&pl::s("'"), &tag, &pl::s("' chunk")])])?;
            }
            if tag.eq_str("LIST_movi") && ee.truthy() {
                if rewind.truthy() && !m(it, &raf, "Seek", vec![rewind.clone(), n(0)])?.truthy() {
                    err = true;
                    continue;
                }
                // save end-of-movie offset so we can seek there if we get errors parsing the movie data
                movi_end = pl::add(&m(it, &raf, "Tell", vec![])?, &len2);
                continue; // parse into movi chunk
            } else if !rewind.truthy() {
                if pl::gt(&len, &n(0x7fffffff)) {
                    if !large_file_support(it, &et)?.truthy() {
                        re(r"([\0-\x1f\x7f-\xff])", "").subst(&mut tag, true, |c| format!("\\x{:02x}", cap(c, 1).to_bytes()[0]).into_bytes());
                        warn(it, &et, cat(&[&pl::s("Stopped parsing at large "), &tag, &pl::s(" chunk (LargeFileSupport not set)")]))?;
                        break;
                    }
                    if large_file_support(it, &et)?.eq_str("2") {
                        warn(it, &et, pl::s("Processing large chunk (LargeFileSupport is 2)"))?;
                    }
                }
                if validate.truthy() && len2.truthy() {
                    // (must actually try to read something after seeking to detect error)
                    let ok = m(it, &raf, "Seek", vec![pl::sub(&len2, &n(1)), n(1)])?.truthy() && pl::num_eq(&read(it, &raf, &mut buff, n(1))?, &n(1));
                    if !ok {
                        err = true;
                        continue;
                    }
                } else if !m(it, &raf, "Seek", vec![len2.clone(), n(1)])?.truthy() {
                    err = true;
                    continue;
                }
            }
        }
        pos = pl::add(&pos, &len2);
    }
    et.delete("DOC_NUM");
    if err {
        warn(it, &et, pl::s("Error reading RIFF file (corrupted?)"))?;
    }
    ret(n(1))
}

pub fn register(it: &mut Interp) {
    it.define_native("Image::ExifTool::RIFF::AUTOLOAD", autoload);
    it.define_native("Image::ExifTool::RIFF::ConvertRIFFDate", convert_riff_date);
    it.define_native("Image::ExifTool::RIFF::ConvertTimecode", convert_timecode);
    it.define_native("Image::ExifTool::RIFF::CalcDuration", calc_duration);
    it.define_native("Image::ExifTool::RIFF::ProcessStreamData", process_stream_data);
    it.define_native("Image::ExifTool::RIFF::MakeTagInfo", make_tag_info);
    it.define_native("Image::ExifTool::RIFF::ProcessChunks", process_chunks);
    it.define_native("Image::ExifTool::RIFF::ProcessSGLT", process_sglt);
    it.define_native("Image::ExifTool::RIFF::ProcessSLLT", process_sllt);
    it.define_native("Image::ExifTool::RIFF::ProcessLucas", process_lucas);
    it.define_native("Image::ExifTool::RIFF::ProcessRIFF", process_riff);
}
