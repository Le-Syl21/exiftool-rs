//! Image::ExifTool::Lytro (Image/ExifTool/Lytro.pm), ported sub by sub (see docs/port/PORTING.md).

use crate::interp::eval::{Cell, ER};
use crate::interp::{Interp, Want};
use crate::modules::exiftool::part2::{check_invocant, get32u};
use crate::modules::random_access::{cell, perl_eval, raf_read_cell};
use crate::perl::{ops, Value};
use crate::pl::{self, arg, cap, n, re, ret};

fn val(c: &Cell) -> Value {
    c.borrow().clone()
}

/// `\$var` of a `my` variable kept in a cell.
fn rf(c: &Cell) -> Value {
    Value::ScalarRef(c.clone())
}

/// `$raf->Read($buff, $len) == $len`
fn read_eq(it: &mut Interp, raf: &Value, buff: &Cell, len: &Value) -> ER<bool> {
    let r = raf_read_cell(it, raf, buff, len.clone())?;
    Ok(pl::num_eq(&r, len))
}

fn seek(it: &mut Interp, raf: &Value, pos: Value, whence: i64) -> ER<bool> {
    Ok(pl::method(it, raf, "Seek", vec![pos, n(whence)])?.truthy())
}

fn warn(it: &mut Interp, et: &Value, msg: &str) -> ER<()> {
    pl::method(it, et, "Warn", vec![pl::s(msg)])?;
    Ok(())
}

fn get_tag_table(it: &mut Interp, name: &str) -> ER<Value> {
    pl::call(it, "Image::ExifTool::GetTagTable", vec![pl::s(name)])
}

/// Extract tags from a parsed JSON hash
/// Inputs: 0) ExifTool ref, 1) tag hash ref, 2) base tag name
pub fn extract_tags(it: &mut Interp, a: Vec<Value>, want: Want) -> ER<Vec<Value>> {
    let (et, meta, parent) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    if pl::ref_of(&meta) != "HASH" {
        warn(it, &et, "Invalid LFP metadata")?;
        return pl::ret_nothing(want);
    }
    let mut tag_table_ptr = Value::Undef;
    for key in pl::call_list(it, "Image::ExifTool::OrderedKeys", vec![meta.clone()])? {
        let tag = pl::concat(&parent, &pl::ucfirst(&key));
        let elem = meta.get(key.to_bytes());
        let vals = if pl::ref_of(&elem) == "ARRAY" { elem.items() } else { vec![elem.clone()] };
        for v in vals {
            if pl::ref_of(&v) == "HASH" {
                extract_tags(it, vec![et.clone(), v, tag.clone()], Want::Void)?;
                continue;
            }
            if !tag_table_ptr.truthy() {
                tag_table_ptr = get_tag_table(it, "Image::ExifTool::Lytro::Main")?;
            }
            if !tag_table_ptr.get(tag.to_bytes()).truthy() {
                let mut name = tag.clone();
                re(r"[^-_a-zA-Z0-9](.?)", "").subst(&mut name, true, |c| pl::uc(&cap(c, 1)).to_bytes().to_vec());
                re(r"ParametersVendorContentComLytroTags", "").subst(&mut name, false, |_| Vec::new());
                let tag_info = Value::new_hash();
                if re(r"^Devices", "").subst(&mut name, false, |_| Vec::new()) == 0 {
                    let groups = Value::new_hash();
                    groups.set("2", pl::s("Image"));
                    tag_info.set("Groups", groups);
                }
                if pl::ref_of(&meta.get(key.to_bytes())) == "ARRAY" {
                    tag_info.set("List", n(1));
                }
                tag_info.set("Name", name.clone());
                let str = if tag.to_bytes() == name.to_bytes() { pl::s("") } else { pl::concat(&pl::s(" as "), &name) };
                let msg = pl::concat(&pl::concat(&pl::concat(&pl::s("  [adding "), &tag), &str), &pl::s("]\n"));
                pl::method(it, &et, "VPrint", vec![n(0), msg])?;
                pl::call(it, "Image::ExifTool::AddTagToTable", vec![tag_table_ptr.clone(), tag.clone(), tag_info])?;
            }
            pl::method(it, &et, "HandleTag", vec![tag_table_ptr.clone(), tag.clone(), v])?;
        }
    }
    pl::ret_nothing(want)
}

/// Process segments from a Lytro LFP image
/// Inputs: 0) ExifTool object reference, 1) dirInfo reference
/// Returns: 1 on success, 0 if this wasn't a valid Lytro image
pub fn process_lfp(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info) = (arg(&a, 0), arg(&a, 1));
    let raf = dir_info.get("RAF");
    let verbose = pl::method(it, &et, "Options", vec![pl::s("Verbose")])?;
    let (buff, id) = (cell(Value::Undef), cell(Value::Undef));

    // validate the Lytro file header
    pl::at(142);
    check_invocant(it, &raf, "Read")?;
    if !(read_eq(it, &raf, &buff, &n(16))? && re(r"^\x89LFP\x0d\x0a\x1a\x0a", "").is_match(&val(&buff))) {
        return ret(n(0));
    }
    pl::method(it, &et, "SetFileType", vec![])?; // set the FileType tag
    pl::call(it, "Image::ExifTool::SetByteOrder", vec![pl::s("MM")])?;
    let tag_table_ptr = get_tag_table(it, "Image::ExifTool::Lytro::Main")?;
    while read_eq(it, &raf, &buff, &n(16))? {
        if !re(r"^\x89LF", "").is_match(&val(&buff)) {
            warn(it, &et, "LFP format error")?;
            break;
        }
        let size = get32u(it, vec![rf(&buff), n(12)], Want::Scalar)?.pop().unwrap_or_default();
        if ops::bit_and(&size, &Value::Int(0x80000000)).truthy() {
            warn(it, &et, "Invalid LFP segment size")?;
            break;
        }
        if !read_eq(it, &raf, &id, &n(80))? {
            warn(it, &et, "Truncated LFP segment")?; // ignore the sha1
            break;
        }
        if verbose.truthy() {
            let mut idv = val(&id);
            re(r"\0.*", "s").subst(&mut idv, false, |_| Vec::new());
            *id.borrow_mut() = idv.clone();
            let msg = pl::concat(&pl::concat(&pl::s(" segment ("), &size), &pl::concat(&pl::s(" bytes, "), &pl::concat(&idv, &pl::s(")\n"))));
            pl::method(it, &et, "VPrint", vec![n(0), pl::substr(&val(&buff), 1, Some(3)), msg])?;
        }
        if pl::gt(&size, &n(20000000)) {
            if !seek(it, &raf, size.clone(), 1)? {
                warn(it, &et, "Seek error in LFP file")?;
                break;
            }
        } else {
            if !read_eq(it, &raf, &buff, &size)? {
                warn(it, &et, "Truncated LFP data")?;
                break;
            }
            let tell = pl::method(it, &raf, "Tell", vec![])?;
            pl::method(it, &et, "VerboseDump", vec![rf(&buff), pl::s("Addr"), pl::sub(&tell, &size)])?;
            if re(r#"^\{\s+""#, "").is_match(&val(&buff)) {
                // JSON metadata?
                perl_eval(it, "pos($buff) = 0", "Image::ExifTool::Lytro", &[("buff", &buff)], Want::Void)?;
                pl::method(it, &et, "HandleTag", vec![tag_table_ptr.clone(), pl::s("JSONMetadata"), val(&buff)])?;
                let meta = pl::call(it, "Image::ExifTool::Import::ReadJSONObject", vec![Value::Undef, rf(&buff)])?;
                extract_tags(it, vec![et.clone(), meta, pl::s("")], Want::Void)?;
            } else if re(r"^\xff\xd8\xff", "").is_match(&val(&buff)) {
                // embedded JPEG image?
                pl::method(it, &et, "HandleTag", vec![tag_table_ptr.clone(), pl::s("EmbeddedImage"), val(&buff)])?;
            }
        }
        // skip padding if necessary
        let pad = pl::sub(&n(16), &pl::modulo(&size, &n(16))?);
        if !pl::num_eq(&pad, &n(16)) {
            seek(it, &raf, pad, 1)?;
        }
    }
    ret(n(1))
}

/// Register the subs ported so far; the others still run from the Perl.
pub fn register(it: &mut Interp) {
    it.define_native("Image::ExifTool::Lytro::ExtractTags", extract_tags);
    it.define_native("Image::ExifTool::Lytro::ProcessLFP", process_lfp);
}
