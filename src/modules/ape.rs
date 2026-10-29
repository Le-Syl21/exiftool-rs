//! Image::ExifTool::APE (Image/ExifTool/APE.pm), ported sub by sub (see docs/port/PORTING.md).
//!
//! Read Monkey's Audio meta information

use crate::interp::eval::ER;
use crate::interp::{Interp, Want};
use crate::modules::random_access::{cell, raf_read_cell, substr};
use crate::perl::Value;
use crate::pl::{self, arg, cap, n, re, ret};

/// `Image::ExifTool::NAME(ARGS)` (the functions imported with :DataAccess :Utils).
fn call(it: &mut Interp, name: &str, args: Vec<Value>) -> ER<Value> {
    pl::call(it, &format!("Image::ExifTool::{name}"), args)
}

/// `\U$1`
fn upper(c: &pl::Caps, i: usize) -> Vec<u8> {
    cap(c, i).to_bytes().to_ascii_uppercase()
}

fn make_tag_impl(it: &mut Interp, tag: &Value, tag_table_ptr: &Value) -> ER<Value> {
    let mut name = pl::ucfirst(&pl::lc(tag));
    // remove invalid characters in tag name and capitalize following letters
    re(r"[^\w-]+(.?)", "s").subst(&mut name, true, |c| upper(c, 1));
    re(r"([a-z0-9])_([a-z])", "").subst(&mut name, true, |c| [&cap(c, 1).to_bytes()[..], &upper(c, 2)[..]].concat());
    let tag_info = Value::new_hash();
    tag_info.set("Name", name);
    if re(r"^Cover Art", "").is_match(tag) && !re(r"Desc$", "").is_match(tag) {
        let g = Value::new_hash();
        g.set("2", pl::s("Preview"));
        tag_info.set("Groups", g);
    }
    call(it, "AddTagToTable", vec![tag_table_ptr.clone(), tag.clone(), tag_info])
}

/// Make tag info hash for specified tag
/// Inputs: 0) tag name, 1) tag table ref
/// - must only call if tag doesn't exist
pub fn make_tag(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    ret(make_tag_impl(it, &arg(&a, 0), &arg(&a, 1))?)
}

/// Extract information from an APE file
/// Inputs: 0) ExifTool object reference, 1) dirInfo reference
/// - Just looks for APE trailer if FileType is already set
/// Returns: 1 on success, 0 if this wasn't a valid APE file
pub fn process_ape(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info) = (arg(&a, 0), arg(&a, 1));

    // must first check for leading/trailing ID3 information
    if !et.get("DoneID3").truthy() {
        crate::runtime::require(it, "Image::ExifTool::ID3")?;
        if pl::call(it, "Image::ExifTool::ID3::ProcessID3", vec![et.clone(), dir_info.clone()])?.truthy() {
            return ret(n(1));
        }
    }
    let raf = dir_info.get("RAF");
    let verbose = pl::method(it, &et, "Options", vec![pl::s("Verbose")])?;
    let buff = cell(Value::Undef);
    let buff_ref = Value::ScalarRef(buff.clone());
    let (mut header, mut tag_table_ptr, mut data_pos, mut old_indent) = (false, Value::Undef, Value::Undef, Value::Undef);
    let b = || buff.borrow().clone();

    et.set("DoneAPE", n(1));

    // check APE signature and process audio information
    // unless this is some other type of file
    if !et.get("FileType").truthy() {
        if !pl::num_eq(&raf_read_cell(it, &raf, &buff, n(32))?, &n(32)) {
            return ret(n(0));
        }
        if !re(r"^(MAC |APETAGEX)", "").is_match(&b()) {
            return ret(n(0));
        }
        pl::method(it, &et, "SetFileType", vec![])?;
        call(it, "SetByteOrder", vec![pl::s("II")])?;

        if re(r"^APETAGEX", "").is_match(&b()) {
            // we already read the APE header
            header = true;
        } else {
            // process the MAC header
            let vers = call(it, "Get16u", vec![buff_ref.clone(), n(4)])?;
            let mut table = Value::Undef;
            if pl::le(&vers, &n(3970)) {
                let s = substr(it, &b(), &n(4), None)?;
                *buff.borrow_mut() = s;
                table = call(it, "GetTagTable", vec![pl::s("Image::ExifTool::APE::OldHeader")])?;
            } else {
                let dlen = call(it, "Get32u", vec![buff_ref.clone(), n(8)])?;
                let hlen = call(it, "Get32u", vec![buff_ref.clone(), n(12)])?;
                if !(dlen.to_i64() & 0x80000000 != 0 || hlen.to_i64() & 0x80000000 != 0)
                    && pl::method(it, &raf, "Seek", vec![dlen.clone(), n(0)])?.truthy()
                    && pl::num_eq(&raf_read_cell(it, &raf, &buff, hlen.clone())?, &hlen)
                {
                    table = call(it, "GetTagTable", vec![pl::s("Image::ExifTool::APE::NewHeader")])?;
                }
            }
            if table.truthy() {
                let d = Value::new_hash();
                d.set("DataPt", buff_ref.clone());
                pl::method(it, &et, "ProcessDirectory", vec![d, table])?;
            }
        }
    }
    // look for APE trailer unless we already found an APE header
    if !header {
        // look for the APE trailer footer...
        let mut foot_pos = n(-32);
        // (...but before the ID3v1 trailer if it exists)
        let done_id3 = et.get("DoneID3");
        if pl::gt(&done_id3, &n(1)) {
            foot_pos = pl::sub(&foot_pos, &done_id3);
        }
        if !pl::method(it, &raf, "Seek", vec![foot_pos, n(2)])?.truthy() {
            return ret(n(1));
        }
        if !pl::num_eq(&raf_read_cell(it, &raf, &buff, n(32))?, &n(32)) {
            return ret(n(1));
        }
        if !re(r"^APETAGEX", "").is_match(&b()) {
            return ret(n(1));
        }
        call(it, "SetByteOrder", vec![pl::s("II")])?;
    }
    //
    // Read the APE data (we have just read the APE header or footer into $buff)
    //
    let u = pl::unpack("x8V4", &b())?;
    let (mut version, mut size, mut count) = (arg(&u, 0), arg(&u, 1), arg(&u, 2));
    version = pl::div(&version, &n(1000))?;
    size = pl::sub(&size, &n(32)); // get size of data only
    if size.to_i64() & 0x80000000 == 0
        && (header || pl::method(it, &raf, "Seek", vec![pl::sub(&pl::neg(&size), &n(32)), n(1)])?.truthy())
        && pl::num_eq(&raf_read_cell(it, &raf, &buff, size.clone())?, &size)
    {
        if verbose.truthy() {
            old_indent = et.get("INDENT");
            et.set("INDENT", pl::concat(&old_indent, &pl::s("| ")));
            pl::method(it, &et, "VerboseDir", vec![pl::concat(&pl::s("APEv"), &version), count.clone(), size.clone()])?;
            let tell = pl::method(it, &raf, "Tell", vec![])?;
            pl::method(it, &et, "VerboseDump", vec![buff_ref.clone(), pl::s("DataPos"), pl::sub(&tell, &size)])?;
        }
        tag_table_ptr = call(it, "GetTagTable", vec![pl::s("Image::ExifTool::APE::Main")])?;
        data_pos = pl::sub(&pl::method(it, &raf, "Tell", vec![])?, &size);
    } else {
        count = n(-1);
    }
    //
    // Process the APE tags
    //
    let mut pos = n(0);
    let special_tags = pl::pkg_hash(it, "Image::ExifTool::specialTags");
    let rx = match crate::perl::regex::compile(br"\G(.*?)\0", "s") {
        Ok(r) => r,
        Err(e) => return crate::interp::eval::die(e),
    };
    let mut i = n(0);
    while pl::lt(&i, &count) {
        // read next APE tag
        if pl::gt(&pl::add(&pos, &n(8)), &size) {
            break;
        }
        let len = call(it, "Get32u", vec![buff_ref.clone(), pos.clone()])?;
        let flags = call(it, "Get32u", vec![buff_ref.clone(), pl::add(&pos, &n(4))])?;
        let bb = b();
        let start = (pos.to_i64() + 8).max(0) as usize;
        let Some(sp) = (if start <= bb.to_bytes().len() { rx.match_at(&bb.to_bytes(), start).ok().flatten() } else { None }) else {
            break;
        };
        let bytes = bb.to_bytes();
        let (s1, e1) = sp[1].unwrap();
        let mut tag = Value::bytes(&bytes[s1..e1]);
        // avoid conflicts with our special table entries
        if special_tags.get(tag.to_bytes()).truthy() {
            tag = pl::concat(&tag, &pl::s("."));
        }
        pos = n(sp[0].unwrap().1 as i64);
        if pl::gt(&pl::add(&pos, &len), &size) {
            break;
        }
        let mut val = substr(it, &bb, &pos, Some(&len))?;
        if !tag_table_ptr.get(tag.to_bytes()).truthy() {
            make_tag_impl(it, &tag, &tag_table_ptr)?;
        }
        // handle binary-value tags
        if flags.to_i64() & 0x06 == 0x02 {
            let mut buf2 = val.clone();
            // extract cover art description separately (hackitty hack)
            if re(r"^Cover Art", "").is_match(&tag) {
                let mut c1 = Value::Undef;
                re(r"^([\x20-\x7e]*)\0", "").subst(&mut buf2, false, |c| {
                    c1 = cap(c, 1);
                    Vec::new()
                });
                if c1.truthy() {
                    let t = pl::concat(&tag, &pl::s(" Desc"));
                    let v = c1;
                    if !tag_table_ptr.get(t.to_bytes()).truthy() {
                        make_tag_impl(it, &t, &tag_table_ptr)?;
                    }
                    pl::method(it, &et, "HandleTag", vec![tag_table_ptr.clone(), t, v])?;
                }
            }
            val = pl::sref(buf2);
        }
        pl::method(
            it,
            &et,
            "HandleTag",
            vec![tag_table_ptr.clone(), tag, val, pl::s("Index"), i.clone(), pl::s("DataPt"), buff_ref.clone(), pl::s("DataPos"), data_pos.clone(), pl::s("Start"), pos.clone(), pl::s("Size"), len.clone()],
        )?;
        pos = pl::add(&pos, &len);
        i = pl::add(&i, &n(1));
    }
    if !pl::num_eq(&i, &count) {
        pl::method(it, &et, "Warn", vec![pl::s("Bad APE trailer")])?;
    }
    if !old_indent.is_undef() {
        et.set("INDENT", old_indent);
    }
    ret(n(1))
}

pub fn register(it: &mut Interp) {
    it.define_native("Image::ExifTool::APE::MakeTag", make_tag);
    it.define_native("Image::ExifTool::APE::ProcessAPE", process_ape);
}
