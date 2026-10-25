//! Image::ExifTool::Vorbis (Image/ExifTool/Vorbis.pm), ported sub by sub (see docs/port/PORTING.md).

use crate::interp::eval::ER;
use crate::interp::{Interp, Want};
use crate::modules::random_access::substr;
use crate::perl::Value;
use crate::pl::{self, arg, cap, n, re, ret};

fn call(it: &mut Interp, name: &str, args: Vec<Value>) -> ER<Value> {
    pl::call(it, &format!("Image::ExifTool::{name}"), args)
}

/// Process Vorbis Comments
/// Inputs: 0) ExifTool object ref, 1) dirInfo ref, 2) tag table ref
/// Returns: 1 on success, otherwise returns 0 and sets a Warning
pub fn process_comments(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info, tag_table_ptr) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    let data_pt = dir_info.get("DataPt");
    let data_pos = dir_info.get("DataPos");
    let pos0 = dir_info.get("DirStart");
    let mut pos = if pos0.truthy() { pos0 } else { n(0) };
    let end = if dir_info.get("DirLen").truthy() { pl::add(&pos, &dir_info.get("DirLen")) } else { pl::length(&data_pt.deref()) };
    let (mut num, mut index) = (Value::Undef, Value::Undef);

    call(it, "SetByteOrder", vec![pl::s("II")])?;
    loop {
        if pl::gt(&pl::add(&pos, &n(4)), &end) {
            break;
        }
        let len = call(it, "Get32u", vec![data_pt.clone(), pos.clone()])?;
        if pl::gt(&pl::add(&pl::add(&pos, &n(4)), &len), &end) {
            break;
        }
        let start = pl::add(&pos, &n(4));
        let buff = substr(it, &data_pt.deref(), &start, Some(&len))?;
        pos = pl::add(&start, &len);
        let (mut tag, val);
        if !num.is_undef() {
            let Some(m) = re(r"(.*?)=(.*)", "s").m(&buff) else { break };
            tag = pl::uc(&cap(&m, 1));
            val = cap(&m, 2);
            // Vorbis tag ID's are all capitals, so they may conflict with our internal tags
            // --> protect against this by adding a trailing underline if necessary
            if pl::pkg_hash(it, "Image::ExifTool::specialTags").get(tag.to_bytes()).truthy() {
                tag = pl::concat(&tag, &pl::s("_"));
            }
        } else {
            tag = pl::s("vendor");
            val = buff.clone();
            num = if pl::lt(&pl::add(&pos, &n(4)), &end) { call(it, "Get32u", vec![data_pt.clone(), pos.clone()])? } else { n(0) };
            let msg = pl::concat(&pl::concat(&pl::s("  + [Vorbis comments with "), &num), &pl::s(" entries]\n"));
            pl::method(it, &et, "VPrint", vec![n(0), msg])?;
            pos = pl::add(&pos, &n(4));
        }
        // add tag to table unless it exists already
        if !tag_table_ptr.get(tag.to_bytes()).truthy() {
            let mut name = pl::ucfirst(&pl::lc(&tag));
            // remove invalid characters in tag name and capitalize following letters
            re(r"[^\w-]+(.?)", "s").subst(&mut name, true, |c| pl::uc(&cap(c, 1)).to_bytes().to_vec());
            re(r"([a-z0-9])_([a-z])", "").subst(&mut name, true, |c| [&cap(c, 1).to_bytes()[..], &pl::uc(&cap(c, 2)).to_bytes()[..]].concat());
            let msg = pl::concat(&pl::concat(&pl::s("  | [adding "), &tag), &pl::s("]\n"));
            pl::method(it, &et, "VPrint", vec![n(0), msg])?;
            let ti = Value::new_hash();
            ti.set("Name", name);
            pl::call(it, "Image::ExifTool::AddTagToTable", vec![tag_table_ptr.clone(), tag.clone(), ti])?;
        }
        let dval = pl::method(it, &et, "Decode", vec![val, pl::s("UTF8")])?;
        pl::method(
            it,
            &et,
            "HandleTag",
            vec![
                tag_table_ptr.clone(),
                tag,
                dval,
                pl::s("Index"),
                index.clone(),
                pl::s("DataPt"),
                data_pt.clone(),
                pl::s("DataPos"),
                data_pos.clone(),
                pl::s("Start"),
                start,
                pl::s("Size"),
                len,
            ],
        )?;
        // all done if this was our last tag
        let was = num.clone();
        num = pl::sub(&num, &n(1));
        if !was.truthy() {
            return ret(n(1));
        }
        index = if !index.is_undef() { pl::add(&index, &n(1)) } else { n(0) };
    }
    pl::method(it, &et, "Warn", vec![pl::s("Format error in Vorbis comments")])?;
    ret(n(0))
}

/// Register the subs ported so far; the others still run from the Perl.
pub fn register(it: &mut Interp) {
    it.define_native("Image::ExifTool::Vorbis::ProcessComments", process_comments);
}
