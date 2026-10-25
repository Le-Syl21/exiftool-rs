//! Image::ExifTool::Radiance (Image/ExifTool/Radiance.pm), ported sub by sub (see docs/port/PORTING.md).

use crate::interp::eval::ER;
use crate::interp::{Interp, Want};
use crate::modules::random_access::raf_read_line;
use crate::perl::Value;
use crate::pl::{self, arg, cap, n, re, ret};

fn call(it: &mut Interp, name: &str, args: Vec<Value>) -> ER<Value> {
    pl::call(it, &format!("Image::ExifTool::{name}"), args)
}

/// `chomp $buff` with `$/` set to "\x0a" (as ProcessHDR has it).
fn chomp_lf(v: &mut Value) {
    let b = v.to_bytes();
    if b.last() == Some(&b'\n') {
        *v = Value::bytes(&b[..b.len() - 1]);
    }
}

/// Extract information from a Radiance HDR file
/// Inputs: 0) ExifTool object reference, 1) DirInfo reference
/// Returns: 1 on success, 0 if this wasn't a valid RGBE image
pub fn process_hdr(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    // local $/ = "\x0a";  # set newline character for reading
    pl::local_global(it, b'$', "main::/", |it| {
        it.set_scalar("main::/", pl::s("\x0a"));
        process_hdr_body(it, a)
    })
}

fn process_hdr_body(it: &mut Interp, a: Vec<Value>) -> ER<Vec<Value>> {
    let (et, dir_info) = (arg(&a, 0), arg(&a, 1));
    let raf = dir_info.get("RAF");
    let mut buff = Value::Undef;

    // verify this is a valid RIFF file
    if !(raf_read_line(it, &raf, &mut buff)?.truthy() && re(r"^#\?(RADIANCE|RGBE)\x0a", "s").is_match(&buff)) {
        return ret(n(0));
    }
    pl::method(it, &et, "SetFileType", vec![])?;
    let tag_table_ptr = call(it, "GetTagTable", vec![pl::s("Image::ExifTool::Radiance::Main")])?;

    while raf_read_line(it, &raf, &mut buff)?.truthy() {
        chomp_lf(&mut buff);
        let l = pl::length(&buff);
        if !(pl::gt(&l, &n(0)) && pl::lt(&l, &n(4096))) {
            break;
        }
        if re(r"^#\s*", "").subst(&mut buff, false, |_| Vec::new()) > 0 {
            if pl::length(&buff).truthy() {
                pl::method(it, &et, "HandleTag", vec![tag_table_ptr.clone(), pl::s("_comment"), buff.clone()])?;
            }
            continue;
        }
        let Some(m) = re(r"^(.*)?\s*=\s*(.*)", "").m(&buff) else {
            if pl::length(&buff).truthy() {
                pl::method(it, &et, "HandleTag", vec![tag_table_ptr.clone(), pl::s("_command"), buff.clone()])?;
            }
            continue;
        };
        // use lower-case tag names to avoid conflicts with reserved tag table entries
        let (tag, val) = (pl::lc(&cap(&m, 1)), cap(&m, 2));
        let mut tag_info = pl::method(it, &et, "GetTagInfo", vec![tag_table_ptr.clone(), tag.clone()])?;
        if !tag_info.truthy() {
            let mut name = tag.clone();
            pl::tr(&mut name, "-_a-zA-Z0-9", "", "dc");
            if !pl::gt(&pl::length(&name), &n(1)) {
                continue;
            }
            name = pl::ucfirst(&name);
            tag_info = Value::new_hash();
            tag_info.set("Name", name);
            pl::call(it, "Image::ExifTool::AddTagToTable", vec![tag_table_ptr.clone(), tag.clone(), tag_info.clone()])?;
        }
        pl::method(it, &et, "FoundTag", vec![tag_info, val])?;
    }
    // get image dimensions
    if raf_read_line(it, &raf, &mut buff)?.truthy() {
        if let Some(m) = re(r"([-+][XY])\s*(\d+)\s*([-+][XY])\s*(\d+)", "").m(&buff) {
            let orient = pl::concat(&pl::concat(&cap(&m, 1), &pl::s(" ")), &cap(&m, 3));
            pl::method(it, &et, "HandleTag", vec![tag_table_ptr.clone(), pl::s("_orient"), orient])?;
            pl::method(it, &et, "FoundTag", vec![pl::s("ImageHeight"), cap(&m, 2)])?;
            pl::method(it, &et, "FoundTag", vec![pl::s("ImageWidth"), cap(&m, 4)])?;
        }
    }
    ret(n(1))
}

/// Register the subs ported so far; the others still run from the Perl.
pub fn register(it: &mut Interp) {
    it.define_native("Image::ExifTool::Radiance::ProcessHDR", process_hdr);
}
