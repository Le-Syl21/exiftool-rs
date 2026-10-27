//! Image::ExifTool::Qualcomm (Image/ExifTool/Qualcomm.pm), ported sub by sub (see docs/port/PORTING.md).
//!
//! Read Qualcomm APP7 meta information

use crate::interp::eval::ER;
use crate::interp::{Interp, Want};
use crate::modules::random_access::{cell, perl_eval1, substr};
use crate::perl::Value;
use crate::pl::{self, arg, cap, n, re, ret};

const MODULE: &str = "Image::ExifTool::Qualcomm";

/// `Image::ExifTool::NAME(ARGS)` (the functions imported with :DataAccess :Utils).
fn call(it: &mut Interp, name: &str, args: Vec<Value>) -> ER<Value> {
    pl::call(it, &format!("Image::ExifTool::{name}"), args)
}

//------------------------------------------------------------------------------
/// Generate tag Name and Description from a Qualcomm tag ID
/// Inputs: 0) tag ID, 1) tagInfo ref
/// Returns: true on success
pub fn make_name_and_desc(_it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    // (local $_: nothing called here sees it, so a plain variable holds it)
    let mut t = arg(&a, 0);
    let tag_info = arg(&a, 1);
    // capitalize various leading acronyms or just first letter...
    if re(r"^(asf|awb|aec|afr|af_|la_|r2_tl|tl)", "").subst(&mut t, false, |c| cap(c, 1).to_bytes().to_ascii_uppercase()) == 0 {
        t = pl::ucfirst(&t);
    }
    re(r"_([a-z])", "").subst(&mut t, true, |c| {
        // capitalize first letter of each word
        let mut v = b"_".to_vec();
        v.extend_from_slice(&pl::ucfirst(&cap(c, 1)).to_bytes());
        v
    });
    re(r"\[(\d+)\]$", "").subst(&mut t, false, |c| pl::sprintf("_%.2d", &[cap(c, 1)]).to_bytes().to_vec()); // use 2-digit subscripts (and remove brackets)
    pl::tr(&mut t, "-_a-zA-Z0-9", "", "dc"); // delete invalid characters
    let mut desc = t.clone();
    // convert underlines to spaces in description
    if pl::tr(&mut desc, "_", " ", "").truthy() {
        // remove unnecessary underlines from tag name...
        re(r"_([A-Z][a-z])", "").subst(&mut t, true, |c| cap(c, 1).to_bytes().to_vec());
        re(r"([a-z0-9])_([A-Z])", "").subst(&mut t, true, |c| [&cap(c, 1).to_bytes()[..], &cap(c, 2).to_bytes()[..]].concat());
        re(r"([A-Za-z])_(\d)", "").subst(&mut t, true, |c| [&cap(c, 1).to_bytes()[..], &cap(c, 2).to_bytes()[..]].concat());
    }
    if !pl::length(&t).truthy() {
        return ret(n(0));
    }
    tag_info.set("Name", t);
    tag_info.set("Description", desc);
    ret(n(1))
}

//------------------------------------------------------------------------------
/// Process Qualcomm Dual Camera APP4 metadata (ref PH)
/// Inputs: 0) ExifTool object ref, 1) dirInfo ref, 2) tag table ref
/// Returns: 1 on success
pub fn process_dual_camera(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info, tag_table_ptr) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    let data_pt = dir_info.get("DataPt");
    let _data_pos = dir_info.get("DataPos");
    let mut pos = dir_info.get("DirStart");

    pl::method(it, &et, "VerboseDir", vec![pl::s("Qualcomm Dual Camera"), Value::Undef, dir_info.get("DirLen")])?;
    // pos($$dataPt) = $pos; while ($$dataPt =~ /\x0a/g) { ... }
    // (the loop ends on a failed match, which resets pos() of the scalar)
    // (pos($$dataPt) = ... autovivifies an undefined $dataPt, but dies on
    // anything else that is not a SCALAR reference)
    if !data_pt.is_undef() {
        pl::at(1327);
        pl::sderef(&data_pt)?;
    }
    let subject = data_pt.deref();
    let b = subject.to_bytes();
    let mut g = pos.to_i64().clamp(0, b.len() as i64) as usize;
    while let Some(i) = b[g.min(b.len())..].iter().position(|&x| x == 0x0a) {
        g = g + i + 1;
        let str_ = substr(it, &subject, &pos, Some(&pl::sub(&pl::sub(&n(g as i64), &pos), &n(1))))?;
        let Some(c) = re(r"^(.*?)\s*=\s*(.*)", "").m(&str_) else { continue };
        let (tag, val) = (cap(&c, 1), cap(&c, 2));
        pl::method(it, &et, "HandleTag", vec![tag_table_ptr.clone(), tag, val, pl::s("MakeTagInfo"), n(1)])?;
        pos = n(g as i64);
    }
    let d = cell(data_pt.clone());
    perl_eval1(it, "pos($$d) = undef", MODULE, &[("d", &d)])?;
    ret(n(1))
}

//------------------------------------------------------------------------------
/// Process Qualcomm APP7 metadata (ref PH)
/// Inputs: 0) ExifTool object ref, 1) dirInfo ref, 2) tag table ref
/// Returns: 1 on success
pub fn process_qualcomm(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info, tag_table_ptr) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    let data_pt = dir_info.get("DataPt");
    let data_pos = dir_info.get("DataPos");
    let mut pos = dir_info.get("DirStart");
    let dir_end = pl::add(&pos, &dir_info.get("DirLen"));

    pl::method(it, &et, "VerboseDir", vec![pl::s("Qualcomm"), Value::Undef, dir_info.get("DirLen")])?;
    call(it, "SetByteOrder", vec![pl::s("II")])?;

    let qualcomm_format = crate::runtime::lex_array(it, MODULE, "qualcommFormat");
    while pl::lt(&pl::add(&pos, &n(3)), &dir_end) {
        let val_len = call(it, "Get16u", vec![data_pt.clone(), pos.clone()])?;
        let tag_len = call(it, "Get8u", vec![data_pt.clone(), pl::add(&pos, &n(2))])?;
        if pl::gt(&pl::add(&pl::add(&pl::add(&pos, &n(8)), &tag_len), &val_len), &dir_end) {
            break;
        }
        pl::at(1357);
        let tag = substr(it, &pl::sderef(&data_pt)?, &pl::add(&pos, &n(3)), Some(&tag_len))?;
        pos = pl::add(&pos, &pl::add(&n(3), &tag_len)); // point to format byte
        let fmt = call(it, "Get8u", vec![data_pt.clone(), pos.clone()])?;
        // (not sure what these counts are for -- both are always 1 in my samples)
        //my $cnt1 = Get16u($dataPt, $pos + 1);
        //my $cnt2 = Get16u($dataPt, $pos + 3);
        pos = pl::add(&pos, &n(5)); // point to start of value data
        let (val, format);
        if pl::le(&fmt, &n(7)) {
            format = qualcomm_format.at(fmt.to_i64());
            val = call(it, "ReadValue", vec![data_pt.clone(), pos.clone(), format.clone(), Value::Undef, val_len.clone()])?;
        } else {
            format = pl::concat(&pl::s("format "), &fmt);
            pl::at(1370);
            let value = substr(it, &pl::sderef(&data_pt)?, &pos, Some(&val_len))?;
            val = pl::sref(value);
        }
        let special = pl::pkg_hash(it, "Image::ExifTool::specialTags");
        if !(!tag_table_ptr.get(tag.to_bytes()).is_undef() || special.get(tag.to_bytes()).truthy()) {
            let tag_info = Value::new_hash();
            if make_name_and_desc(it, vec![tag.clone(), tag_info.clone()], Want::Scalar)?[0].truthy() {
                let msg = pl::concat(&pl::concat(&pl::s("[adding Qualcomm:"), &tag_info.get("Name")), &pl::s("]\n"));
                pl::method(it, &et, "VPrint", vec![n(0), et.get("INDENT"), msg])?;
                call(it, "AddTagToTable", vec![tag_table_ptr.clone(), tag.clone(), tag_info])?;
            }
        }
        pl::method(
            it,
            &et,
            "HandleTag",
            vec![
                tag_table_ptr.clone(),
                tag,
                val,
                pl::s("DataPt"),
                data_pt.clone(),
                pl::s("DataPos"),
                data_pos.clone(),
                pl::s("Start"),
                pos.clone(),
                pl::s("Size"),
                val_len.clone(),
                pl::s("Format"),
                format,
            ],
        )?;
        pos = pl::add(&pos, &val_len); // point to start of next entry
    }
    ret(n(1))
}

/// Register the subs ported so far; the others still run from the Perl.
pub fn register(it: &mut Interp) {
    it.define_native("Image::ExifTool::Qualcomm::MakeNameAndDesc", make_name_and_desc);
    it.define_native("Image::ExifTool::Qualcomm::ProcessDualCamera", process_dual_camera);
    it.define_native("Image::ExifTool::Qualcomm::ProcessQualcomm", process_qualcomm);
}
