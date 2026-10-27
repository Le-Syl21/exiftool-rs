//! Image::ExifTool::PPM (Image/ExifTool/PPM.pm), ported sub by sub (see docs/port/PORTING.md).
//!
//! Read/write PPM meta information

use crate::interp::eval::{Cell, ER};
use crate::interp::{Interp, Want};
use crate::modules::random_access::{cell, raf_read_cell, substr};
use crate::perl::regex as pre;
use crate::perl::Value;
use crate::pl::{self, arg, cap, n, re, ret, Caps};

/// `$raf->Read(...)` runs as a perl statement (random_access::perl_eval),
/// whose errors would name "(eval N)": an invocant that is not an object
/// gives perl's error here instead, at the native's location.
fn need_obj(v: &Value, method: &str) -> ER<()> {
    match v {
        Value::Undef => crate::interp::eval::die(format!("Can't call method \"{method}\" on an undefined value")),
        Value::HashRef(h) if h.borrow().blessed.is_none() => crate::interp::eval::die(format!("Can't call method \"{method}\" on unblessed reference")),
        Value::ArrayRef(_) | Value::ScalarRef(_) | Value::Code(_) => crate::interp::eval::die(format!("Can't call method \"{method}\" on unblessed reference")),
        _ => Ok(()),
    }
}

/// `Image::ExifTool::NAME(ARGS)` (the functions imported with :DataAccess :Utils).
fn call(it: &mut Interp, name: &str, args: Vec<Value>) -> ER<Value> {
    pl::call(it, &format!("Image::ExifTool::{name}"), args)
}

/// `$raf->Read($buff, $len)` into the `my $buff` cell.
fn read(it: &mut Interp, raf: &Value, buff: &Cell, len: &Value) -> ER<Value> {
    raf_read_cell(it, raf, buff, len.clone())
}

fn val(c: &Cell) -> Value {
    c.borrow().clone()
}

/// `$buff =~ /PAT/g` (or `/gc` when `keep`) in scalar context, with `pos`
/// kept in `pos` (None is undef): a match starts at pos and moves it to its
/// end; a failure resets pos, except with /c.
fn match_g(subject: &Value, pat: &str, pos: &mut Option<usize>, keep: bool) -> Option<Caps> {
    let rx = pre::compile(pat.as_bytes(), "").unwrap_or_else(|e| panic!("bad pattern /{pat}/: {e}"));
    let b = subject.to_bytes();
    match rx.match_at(&b, pos.unwrap_or(0)) {
        Ok(Some(sp)) => {
            *pos = sp[0].map(|(_, e)| e);
            Some(sp.iter().map(|g| g.map(|(s, e)| Value::bytes(&b[s..e]))).collect())
        }
        _ => {
            if !keep {
                *pos = None;
            }
            None
        }
    }
}

/// Read or write information in a PPM/PGM/PBM image
/// Inputs: 0) ExifTool object reference, 1) Directory information reference
/// Returns: 1 on success, 0 if this wasn't a valid PPM file, -1 on write error
pub fn process_ppm(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info) = (arg(&a, 0), arg(&a, 1));
    let raf = dir_info.get("RAF");
    let outfile = dir_info.get("OutFile");
    let verbose = pl::method(it, &et, "Options", vec![pl::s("Verbose")])?;
    let out = pl::method(it, &et, "Options", vec![pl::s("TextOut")])?;
    let buff = cell(Value::Undef);
    let (mut num, mut type_);
    let info = Value::new_hash();
    let mut seal = false;
    // pos($buff): reset by every change to $buff
    let mut pos: Option<usize>;
    //
    // read as much of the image as necessary to extract the header and comments
    //
    loop {
        if !val(&buff).is_undef() {
            // need to read some more data
            let tmp = cell(Value::Undef);
            if !read(it, &raf, &tmp, &n(1024))?.truthy() {
                return ret(n(0));
            }
            let b = pl::concat(&val(&buff), &val(&tmp));
            *buff.borrow_mut() = b;
        } else {
            pl::at(43);
            need_obj(&raf, "Read")?;
            if !read(it, &raf, &buff, &n(1024))?.truthy() {
                return ret(n(0));
            }
        }
        pos = None;
        let b = val(&buff);
        // verify this is a valid PPM file
        let Some(m) = match_g(&b, r"^P([1-6])\s+", &mut pos, false) else {
            return ret(n(0));
        };
        num = cap(&m, 1);
        // note: may contain comments starting with '#'
        if match_g(&b, r"\G#", &mut pos, true).is_some() {
            // must read more if we are in the middle of a comment
            let Some(m) = match_g(&b, r"\G ?(.*[\n\r]+(#.*[\n\r]+)*)\s*", &mut pos, false) else {
                continue;
            };
            info.set("Comment", cap(&m, 1));
            if match_g(&b, r"\G#", &mut pos, true).is_some() {
                continue;
            }
        } else {
            info.delete("Comment");
        }
        let Some(m) = match_g(&b, r"\G(\S+)\s+(\S+)\s+", &mut pos, false) else {
            continue;
        };
        info.set("ImageWidth", cap(&m, 1));
        info.set("ImageHeight", cap(&m, 2));
        type_ = pl::s(["PPM", "PBM", "PGM"][pl::modulo(&num, &n(3))?.to_i64() as usize]);
        if type_.eq_str("PBM") {
            break; // (no MaxVal for PBM images)
        }
        if match_g(&b, r"\G\s*#", &mut pos, true).is_some() {
            let Some(m) = match_g(&b, r"\G ?(.*[\n\r]+(#.*[\n\r]+)*)\s*", &mut pos, false) else {
                continue;
            };
            if !info.exists("Comment") {
                info.set("Comment", pl::s(""));
            }
            info.set("Comment", pl::concat(&info.get("Comment"), &cap(&m, 1)));
            if match_g(&b, r"\G#", &mut pos, true).is_some() {
                continue;
            }
        }
        let Some(m) = match_g(&b, r"\G(\S+)\s", &mut pos, false) else {
            continue;
        };
        info.set("MaxVal", cap(&m, 1));
        break;
    }
    // validate numerical values
    for k in info.keys() {
        if &k[..] == b"Comment" {
            continue;
        }
        if !re(r"^\d+$", "").is_match(&info.get(&k)) {
            return ret(n(0));
        }
    }
    if !info.get("Comment").is_undef() {
        let mut c = info.get("Comment");
        re(r"^# ?", "m").subst(&mut c, true, |_| Vec::new()); // remove "# " at the start of each line
        re(r"[\n\r]+$", "").subst(&mut c, false, |_| Vec::new()); // remove trailing newline
        info.set("Comment", c.clone());
        if re(r"^<seal seal=", "").is_match(&c) {
            seal = true;
        }
    }
    pl::method(it, &et, "SetFileType", vec![type_.clone()])?;
    let len = match pos {
        Some(p) => n(p as i64),
        None => Value::Undef,
    };
    //
    // rewrite the file if requested
    //
    if outfile.truthy() {
        let nv_hash = pl::sref(Value::Undef);
        let mut new_comment = pl::method(it, &et, "GetNewValue", vec![pl::s("Comment"), nv_hash.clone()])?;
        let nv_hash = nv_hash.deref();
        let old_comment = info.get("Comment");
        if pl::method(it, &et, "IsOverwriting", vec![nv_hash, old_comment.clone()])?.truthy() {
            et.set("CHANGED", pl::add(&et.get("CHANGED"), &n(1)));
            if !old_comment.is_undef() {
                pl::method(it, &et, "VerboseValue", vec![pl::s("- Comment"), old_comment.clone()])?;
            }
            if !new_comment.is_undef() {
                pl::method(it, &et, "VerboseValue", vec![pl::s("+ Comment"), new_comment.clone()])?;
            }
        } else if seal && et.get("DEL_GROUP").get("SEAL").truthy() {
            // delete SEAL comment
            pl::method(it, &et, "VerboseValue", vec![pl::s("- Comment"), old_comment.clone()])?;
            et.set("CHANGED", pl::add(&et.get("CHANGED"), &n(1)));
        } else {
            new_comment = old_comment; // use existing comment
        }
        let mut hdr = pl::concat(&pl::concat(&pl::s("P"), &num), &pl::s("\n"));
        if !new_comment.is_undef() {
            re(r"\n", "").subst(&mut new_comment, true, |_| b"\n# ".to_vec());
            hdr = pl::concat(&hdr, &pl::concat(&pl::concat(&pl::s("# "), &new_comment), &pl::s("\n")));
        }
        hdr = pl::concat(&hdr, &pl::concat(&pl::concat(&pl::concat(&info.get("ImageWidth"), &pl::s(" ")), &info.get("ImageHeight")), &pl::s("\n")));
        if !type_.eq_str("PBM") {
            hdr = pl::concat(&hdr, &pl::concat(&info.get("MaxVal"), &pl::s("\n")));
        }
        // write header and start of image
        let rest = substr(it, &val(&buff), &len, None)?;
        if !call(it, "Write", vec![outfile.clone(), hdr, rest])?.truthy() {
            return ret(n(-1));
        }
        // copy over the rest of the image
        while read(it, &raf, &buff, &n(0x10000))?.truthy() {
            if !call(it, "Write", vec![outfile.clone(), val(&buff)])?.truthy() {
                return ret(n(-1));
            }
        }
        return ret(n(1));
    }
    //
    // save extracted information
    //
    if pl::gt(&verbose, &n(2)) {
        let line = pl::concat(&pl::concat(&pl::concat(&type_, &pl::s(" header (")), &len), &pl::s(" bytes):\n"));
        pl::print_to(it, Some(&out), &[line])?;
        pl::method(it, &et, "VerboseDump", vec![Value::ScalarRef(buff.clone()), pl::s("Len"), len.clone()])?;
    }
    if seal {
        let d = Value::new_hash();
        d.set("DataPt", pl::elem_ref(&info, b"Comment"));
        let t = call(it, "GetTagTable", vec![pl::s("Image::ExifTool::XMP::SEAL")])?;
        pl::method(it, &et, "ProcessDirectory", vec![d, t])?;
        info.delete("Comment");
    }
    for tag in ["Comment", "ImageWidth", "ImageHeight", "MaxVal"] {
        if !info.get(tag).is_undef() {
            pl::method(it, &et, "FoundTag", vec![pl::s(tag), info.get(tag)])?;
        }
    }
    ret(n(1))
}

/// Register the subs ported so far; the others still run from the Perl.
pub fn register(it: &mut Interp) {
    it.define_native("Image::ExifTool::PPM::ProcessPPM", process_ppm);
}
