//! Image::ExifTool::Olympus (Image/ExifTool/Olympus.pm), ported sub by sub (see docs/port/PORTING.md).

use crate::interp::eval::ER;
use crate::interp::{Interp, Want};
use crate::modules::random_access::raf_read;
use crate::perl::Value;
use crate::pl::{self, arg, cap, n, re, ret};

/// `hex($s)` as perl's builtin: an optional "0x"/"x" prefix, underscores
/// skipped, stops at the first other character.
pub(crate) fn hex(v: &Value) -> Value {
    let b = v.to_bytes();
    let mut s: &[u8] = &b;
    if s.starts_with(b"0x") || s.starts_with(b"0X") {
        s = &s[2..];
    } else if s.starts_with(b"x") || s.starts_with(b"X") {
        s = &s[1..];
    }
    let mut u: u64 = 0;
    let mut f: f64 = 0.0;
    let mut overflow = false;
    for &c in s {
        if c == b'_' {
            continue;
        }
        let Some(d) = (c as char).to_digit(16) else { break };
        if !overflow {
            match u.checked_mul(16).and_then(|x| x.checked_add(d as u64)) {
                Some(x) => u = x,
                None => {
                    overflow = true;
                    f = u as f64 * 16.0 + d as f64;
                }
            }
        } else {
            f = f * 16.0 + d as f64;
        }
    }
    if overflow {
        Value::Num(f)
    } else {
        crate::perl::ops::from_uv(u)
    }
}

//------------------------------------------------------------------------------
/// Determine if the extender (EX-25/EC-14) was really attached (ref 9)
/// Inputs: 0) Extender, 1) LensType string, 2) MaxApertureAtMaxFocal
/// Returns: 0=not attached, 1=attached, 2=could have been removed
/// Notes: Olympus has a bug in the in-camera firmware which results in the
/// extender information being cached and written into the EXIF data even after
/// the extender has been removed.  You must power cycle the camera to prevent it
/// from writing the cached extender information into the EXIF data.
pub fn extender_status(_it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (extender, lens_type, max_aperture) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    let info = pl::split_ws(&extender);
    // validate that extender identifier is reasonable
    if !(info.len() >= 2 && hex(&info[1]).truthy()) {
        return ret(n(0));
    }
    // if it's not an EC-14 (id '0 04') then assume it was really attached
    // (other extenders don't seem to affect the reported max aperture)
    if !pl::str_eq(&pl::concat(&pl::concat(&info[0], &pl::s(" ")), &info[1]), "0 04") {
        return ret(n(1));
    }
    // get the maximum aperture for this lens (in $1)
    let Some(m) = re(r" F(\d+(\.\d+)?)", "").m(&lens_type) else { return ret(n(1)) };
    // If the maximum aperture at the maximum focal length is greater than the
    // known max/max aperture of the lens, then the extender must be attached
    ret(if pl::gt(&pl::sub(&max_aperture, &cap(&m, 1)), &Value::Num(0.2)) { n(1) } else { n(2) })
}

//------------------------------------------------------------------------------
/// Print AF points
/// Inputs: 0) AF point data (string of integers)
/// Notes: I'm just guessing that the 2nd and 4th bytes are the Y coordinates,
/// and that more AF points will show up in the future (derived from E-1 images,
/// and the E-1 uses just one of 3 possible AF points, all centered in Y) - PH
pub fn print_af_areas(_it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let mut val = arg(&a, 0);
    let points = pl::split_ws(&val);
    // %afPointNames (keys are the numeric keys stringified)
    let af_point_names = |pt: &Value| -> Option<&'static str> {
        let k = pt.to_bytes();
        [(0x36794285i64, "Left"), (0x79798585, "Center"), (0xBD79C985, "Right")].iter().find(|(id, _)| *k == *n(*id).to_bytes()).map(|(_, name)| *name)
    };
    val = pl::s("");
    for pt in &points {
        if !pt.truthy() {
            continue;
        }
        if val.truthy() {
            val = pl::concat(&val, &pl::s(", "));
        }
        if let Some(name) = af_point_names(pt) {
            val = pl::concat(&val, &pl::s(&format!("{name} ")));
        }
        let coords = pl::unpack("C4", &pl::pack("N", &[pt.clone()])?)?;
        let c = |i: usize| arg(&coords, i);
        let s = pl::concat(
            &pl::concat(&pl::concat(&pl::concat(&pl::s("("), &c(0)), &pl::concat(&pl::s(","), &c(1))), &pl::concat(&pl::s(")-("), &c(2))),
            &pl::concat(&pl::concat(&pl::s(","), &c(3)), &pl::s(")")),
        );
        val = pl::concat(&val, &s);
    }
    if !val.truthy() {
        val = pl::s("none");
    }
    ret(val)
}

//------------------------------------------------------------------------------
/// Extract information from a DSS/DS2 voice recorder audio file or ID3 XOLY frame
/// Inputs: 0) ExifTool object reference, 1) dirInfo reference
/// Returns: 1 on success
pub fn process_dss(it: &mut Interp, a: Vec<Value>, want: Want) -> ER<Vec<Value>> {
    let (et, mut dir_info) = (arg(&a, 0), arg(&a, 1));

    // allow this to be called with either RAF or DataPt
    let raf = dir_info.get("RAF");
    if raf.truthy() {
        let mut buff = Value::Undef;
        if !pl::gt(&raf_read(it, &raf, &mut buff, n(898))?, &n(68)) {
            return ret(n(0));
        }
        if !re(r"^(\x02dss|\x03ds2)", "").is_match(&buff) {
            return ret(n(0));
        }
        dir_info = Value::new_hash();
        dir_info.set("DataPt", pl::sref(buff.clone()));
        pl::method(it, &et, "SetFileType", vec![pl::uc(&pl::substr(&buff, 1, Some(3)))])?;
    }
    let tag_table_ptr = pl::call(it, "Image::ExifTool::GetTagTable", vec![pl::s("Image::ExifTool::Olympus::DSS")])?;
    it.call_method(&et, "ProcessBinaryData", vec![et.clone(), dir_info, tag_table_ptr], want)
}

//------------------------------------------------------------------------------
/// Process ORF file
/// Inputs: 0) ExifTool object reference, 1) directory information reference
/// Returns: 1 if this looked like a valid ORF file, 0 otherwise
pub fn process_orf(it: &mut Interp, a: Vec<Value>, want: Want) -> ER<Vec<Value>> {
    let (et, dir_info) = (arg(&a, 0), arg(&a, 1));
    it.call_method(&et, "ProcessTIFF", vec![et.clone(), dir_info], want)
}

/// Register the subs ported so far; the others still run from the Perl.
pub fn register(it: &mut Interp) {
    it.define_native("Image::ExifTool::Olympus::ExtenderStatus", extender_status);
    it.define_native("Image::ExifTool::Olympus::PrintAFAreas", print_af_areas);
    it.define_native("Image::ExifTool::Olympus::ProcessDSS", process_dss);
    it.define_native("Image::ExifTool::Olympus::ProcessORF", process_orf);
}
