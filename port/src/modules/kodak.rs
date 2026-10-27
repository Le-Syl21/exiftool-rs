//! Image::ExifTool::Kodak (Image/ExifTool/Kodak.pm), ported sub by sub (see docs/port/PORTING.md).

use crate::interp::eval::ER;
use crate::interp::{Interp, Want};
use crate::modules::exiftool::part2;
use crate::modules::random_access::{cell, substr};
use crate::perl::{ops, Value};
use crate::pl::{self, arg, cap, n, re, ret};

fn g32u(it: &mut Interp, data_pt: &Value, pos: Value) -> ER<Value> {
    Ok(part2::get32u(it, vec![data_pt.clone(), pos], Want::Scalar)?.pop().unwrap_or_default())
}

fn rat64s(it: &mut Interp, data_pt: &Value, pos: Value) -> ER<Value> {
    Ok(part2::get_rational64s(it, vec![data_pt.clone(), pos], Want::Scalar)?.pop().unwrap_or_default())
}

/// `Image::ExifTool::GetRational64s($dataPt, $pos) . ' ' . ... ($pos + 8) . ' ' . ... ($pos + 16)`
fn three_rationals(it: &mut Interp, data_pt: &Value, pos: &Value) -> ER<Value> {
    let r0 = rat64s(it, data_pt, pos.clone())?;
    let r1 = rat64s(it, data_pt, pl::add(pos, &n(8)))?;
    let r2 = rat64s(it, data_pt, pl::add(pos, &n(16)))?;
    Ok(pl::join(" ", &[r0, r1, r2]))
}

//------------------------------------------------------------------------------
/// Process Kodak accelerometer data (ref PH)
/// Inputs: 0) ExifTool ref, 1) dirInfo ref, 2) tag table ref
/// Returns: 1 on success
pub fn process_pose(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info, tag_table_ptr) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    let data_pt = dir_info.get("DataPt");
    pl::at(3080);
    let dir_len = pl::length(&pl::sderef(&data_pt)?);
    let ee = pl::method(it, &et, "Options", vec![pl::s("ExtractEmbedded")])?;
    let (mut i, mut pos);

    if pl::lt(&dir_len, &n(0x14)) {
        return ret(n(0));
    }
    let mut num = g32u(it, &data_pt, n(0x10))?;
    if pl::lt(&dir_len, &pl::add(&n(0x14), &pl::mul(&num, &n(24)))) {
        return ret(n(0));
    }

    pl::method(it, &et, "VerboseDir", vec![pl::s("Kodak pose"), Value::Undef, dir_len.clone()])?;

    et.set("DOC_NUM", n(0));
    i = n(0);
    pos = n(0x14);
    while pl::lt(&i, &num) {
        let v = three_rationals(it, &data_pt, &pos)?;
        pl::method(it, &et, "HandleTag", vec![tag_table_ptr.clone(), pl::s("AngularVelocity"), v])?;
        if !ee.truthy() {
            pos = pl::add(&pos, &pl::mul(&num, &n(24)));
            break;
        }
        et.set("DOC_NUM", pl::add(&et.get("DOC_NUM"), &n(1)));
        i = pl::add(&i, &n(1));
        pos = pl::add(&pos, &n(24));
    }
    et.set("DOC_NUM", n(0));

    if pl::lt(&dir_len, &pl::add(&pos, &n(0x10))) {
        return ret(n(1));
    }
    num = g32u(it, &data_pt, pl::add(&pos, &n(0x0c)))?;
    if pl::lt(&dir_len, &pl::add(&pl::add(&pos, &n(0x10)), &pl::mul(&num, &n(24)))) {
        return ret(n(1));
    }

    i = n(0);
    pos = pl::add(&pos, &n(0x10));
    while pl::lt(&i, &num) {
        let v = three_rationals(it, &data_pt, &pos)?;
        pl::method(it, &et, "HandleTag", vec![tag_table_ptr.clone(), pl::s("Accelerometer"), v])?;
        if !ee.truthy() {
            pos = pl::add(&pos, &pl::mul(&num, &n(24)));
            break;
        }
        et.set("DOC_NUM", pl::add(&et.get("DOC_NUM"), &n(1)));
        i = pl::add(&i, &n(1));
        pos = pl::add(&pos, &n(24));
    }
    let _ = pos;
    et.set("DOC_NUM", n(0));
    if !ee.truthy() {
        pl::method(it, &et, "Warn", vec![pl::s("Use the ExtractEmbedded option to extract all accelerometer data"), n(3)])?;
    }
    ret(n(1))
}

//------------------------------------------------------------------------------
/// Calculate RGB levels from associated tags (ref 3)
/// Inputs: 0) KodakIFD:WhiteBalance, 1-4) WB_RGBMul0-3, 5-8) WB_RGBCoeffs0-3
///         9) (optional) KodakIFD:ColorTemperature, 10) (optional) Kodak:WB_RGBLevels
/// Returns: WB_RGBLevels or undef
pub fn calculate_rgb_levels(_it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    if arg(&a, 10).truthy() {
        return pl::ret_undef(); // use existing software levels if they exist
    }
    let wbi = arg(&a, 0);
    if pl::lt(&wbi, &n(0)) || pl::gt(&wbi, &n(3)) {
        return pl::ret_undef();
    }
    let idx = |off: i64| -> Value {
        let i = pl::add(&wbi, &n(off)).to_i64();
        if i < 0 {
            Value::Undef
        } else {
            arg(&a, i as usize)
        }
    };
    // split ' ', $_[$wbi + 1], 13 (only use the first 12 coeffs)
    let m = idx(1).to_bytes();
    let lead = m.iter().take_while(|c| c.is_ascii_whitespace()).count();
    let mul = pl::split(&re(r"\s+", ""), &Value::bytes(&m[lead..]), 13);
    pl::at(3129);
    let coefs = pl::split_ws(&pl::sderef(&idx(5))?); // (extra de-reference for Binary data)
    let t9 = arg(&a, 9);
    let wbtemp100 = pl::div(&if t9.truthy() { t9 } else { n(6500) }, &n(100))?;
    if !(mul.len() >= 3 && coefs.len() >= 12) {
        return pl::ret_undef();
    }
    let mut cam_mul: Vec<Value> = Vec::new();
    let mut nn = 0usize;
    for c in 0..3 {
        let mut num = n(0);
        for i in 0..4 {
            num = pl::add(&num, &pl::mul(&coefs[nn], &ops::pow(&wbtemp100, &n(i))));
            nn += 1;
        }
        pl::at(3137);
        cam_mul.push(pl::div(&n(2048), &pl::mul(&num, &mul[c]))?);
    }
    ret(pl::join(" ", &cam_mul))
}

//------------------------------------------------------------------------------
/// Process Kodak textual TextualInfo
/// Inputs: 0) ExifTool object ref, 1) dirInfo hash ref, 2) tag table ref
/// Returns: 1 on success
pub fn process_kodak_text(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info, tag_table_ptr) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    let data_pt = dir_info.get("DataPt");
    let ds = dir_info.get("DirStart");
    let dir_start = if ds.truthy() { ds } else { n(0) };
    let dl = dir_info.get("DirLen");
    let dir_len = if dl.truthy() {
        dl
    } else {
        pl::at(3151);
        pl::sub(&pl::length(&pl::sderef(&data_pt)?), &dir_start)
    };
    pl::at(3152);
    let mut data = substr(it, &pl::sderef(&data_pt)?, &dir_start, Some(&dir_len))?;
    re(r"\0.*", "s").subst(&mut data, false, |_| Vec::new()); // truncate at null if it exists
    let lines = pl::split(&re(r"[\n\r]+", ""), &data, 0);
    let (mut success, mut other) = (Value::Undef, Vec::<Value>::new());
    let mut tag_info;
    pl::method(it, &et, "VerboseDir", vec![pl::s("Kodak Text")])?;
    for line in &lines {
        if let Some(m) = re(r"(.*?):\s*(.*)", "").m(line) {
            let (tag, val) = (cap(&m, 1), cap(&m, 2));
            if tag_table_ptr.get(tag.to_bytes()).truthy() {
                tag_info = pl::method(it, &et, "GetTagInfo", vec![tag_table_ptr.clone(), tag.clone()])?;
            } else {
                let mut tag_name = tag.clone();
                re(r"([A-Z])\s+([A-Za-z])", "").subst(&mut tag_name, true, |c| [&cap(c, 1).to_bytes()[..], b"_", &pl::uc(&cap(c, 2)).to_bytes()[..]].concat());
                re(r"([a-z])\s+([A-Za-z0-9])", "").subst(&mut tag_name, true, |c| [&cap(c, 1).to_bytes()[..], &pl::uc(&cap(c, 2)).to_bytes()[..]].concat());
                re(r"\s+", "").subst(&mut tag_name, true, |_| Vec::new());
                re(r"[^-\w]+", "").subst(&mut tag_name, true, |_| Vec::new()); // delete remaining invalid characters
                if !tag_name.truthy() {
                    tag_name = pl::s("NoName");
                }
                tag_info = Value::new_hash();
                tag_info.set("Name", tag_name);
                pl::call(it, "Image::ExifTool::AddTagToTable", vec![tag_table_ptr.clone(), tag.clone(), tag_info.clone()])?;
            }
            pl::method(it, &et, "HandleTag", vec![tag_table_ptr.clone(), tag, val, pl::s("TagInfo"), tag_info.clone()])?;
            success = n(1);
        } else {
            // strip off leading/trailing white space and ignore blank lines
            if let Some(m) = re(r"^\s*(\S.*?)\s*$", "").m(line) {
                other.push(cap(&m, 1));
            }
        }
    }
    if success.truthy() {
        if !other.is_empty() {
            tag_info = pl::method(it, &et, "GetTagInfo", vec![tag_table_ptr.clone(), pl::s("_other_info")])?;
            pl::method(it, &et, "FoundTag", vec![tag_info, Value::new_array(other)])?;
        }
    } else {
        pl::method(it, &et, "Warn", vec![pl::s("Can't parse Kodak TextualInfo data"), n(1)])?;
    }
    ret(success)
}

//------------------------------------------------------------------------------
/// Process Kodak IFD (with leading byte order mark)
/// Inputs: 0) ExifTool object ref, 1) dirInfo hash ref, 2) tag table ref
/// Returns: 1 on success, otherwise returns 0 and sets a Warning
pub fn process_kodak_ifd(it: &mut Interp, a: Vec<Value>, want: Want) -> ER<Vec<Value>> {
    let (et, dir_info, tag_table_ptr) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    let ds = dir_info.get("DirStart");
    let dir_start = if ds.truthy() { ds } else { n(0) };
    if pl::le(&dir_start, &n(0)) || pl::gt(&pl::add(&dir_start, &n(2)), &dir_info.get("DataLen")) {
        return ret(n(1));
    }
    pl::at(3199);
    let byte_order = substr(it, &pl::sderef(&dir_info.get("DataPt"))?, &dir_start, Some(&n(2)))?;
    if !pl::call(it, "Image::ExifTool::SetByteOrder", vec![byte_order])?.truthy() {
        let msg = pl::concat(&pl::concat(&pl::s("Invalid Kodak "), &dir_info.get("Name")), &pl::s(" directory"));
        pl::method(it, &et, "Warn", vec![msg])?;
        return ret(n(1));
    }
    dir_info.set("DirStart", pl::add(&dir_info.get("DirStart"), &n(2))); // skip byte order mark
    dir_info.set("DirLen", pl::sub(&dir_info.get("DirLen"), &n(2)));
    if et.get("HTML_DUMP").truthy() {
        let base = pl::add(&dir_info.get("Base"), &dir_info.get("DataPos"));
        pl::method(it, &et, "HDump", vec![pl::add(&dir_start, &base), n(2), pl::s("Byte Order Mark")])?;
    }
    it.call_named("Image::ExifTool::Exif::ProcessExif", vec![et, dir_info, tag_table_ptr], want)
}

//------------------------------------------------------------------------------
/// Write Kodak IFD (with leading byte order mark)
/// Inputs: 0) ExifTool object ref, 1) source dirInfo ref, 2) tag table ref
/// Returns: Exif data block (may be empty if no Exif data) or undef on error
pub fn write_kodak_ifd(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info, tag_table_ptr) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    let ds = dir_info.get("DirStart");
    let dir_start = if ds.truthy() { ds } else { n(0) };
    if pl::le(&dir_start, &n(0)) || pl::gt(&pl::add(&dir_start, &n(2)), &dir_info.get("DataLen")) {
        return ret(pl::s(""));
    }
    pl::at(3222);
    let byte_order = substr(it, &pl::sderef(&dir_info.get("DataPt"))?, &dir_start, Some(&n(2)))?;
    if !pl::call(it, "Image::ExifTool::SetByteOrder", vec![byte_order])?.truthy() {
        return ret(pl::s(""));
    }
    dir_info.set("DirStart", pl::add(&dir_info.get("DirStart"), &n(2))); // skip byte order mark
    dir_info.set("DirLen", pl::sub(&dir_info.get("DirLen"), &n(2)));
    let buff = pl::call(it, "Image::ExifTool::Exif::WriteExif", vec![et, dir_info.clone(), tag_table_ptr])?;
    if !(!buff.is_undef() && pl::length(&buff).truthy()) {
        return ret(buff);
    }
    let buff = cell(buff);
    // apply one-time fixup for length of byte order mark
    if dir_info.get("Fixup").truthy() {
        let fixup = dir_info.get("Fixup");
        fixup.set("Shift", pl::add(&fixup.get("Shift"), &n(2)));
        pl::method(it, &fixup, "ApplyFixup", vec![Value::ScalarRef(buff.clone())])?;
        dir_info.delete("Fixup");
    }
    let bo = pl::call(it, "Image::ExifTool::GetByteOrder", vec![])?;
    let b = buff.borrow().clone();
    ret(pl::concat(&bo, &b))
}

/// Register the subs ported so far; the others still run from the Perl.
pub fn register(it: &mut Interp) {
    it.define_native("Image::ExifTool::Kodak::ProcessPose", process_pose);
    it.define_native("Image::ExifTool::Kodak::CalculateRGBLevels", calculate_rgb_levels);
    it.define_native("Image::ExifTool::Kodak::ProcessKodakText", process_kodak_text);
    it.define_native("Image::ExifTool::Kodak::ProcessKodakIFD", process_kodak_ifd);
    it.define_native("Image::ExifTool::Kodak::WriteKodakIFD", write_kodak_ifd);
}
