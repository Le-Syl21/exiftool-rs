//! Image::ExifTool::Matroska (Image/ExifTool/Matroska.pm), ported sub by sub (see docs/port/PORTING.md).
//!
//! Read meta information from Matroska multimedia files

use crate::interp::eval::ER;
use crate::interp::{Interp, Want};
use crate::modules::random_access::{cell, raf_read_cell, substr};
use crate::perl::Value;
use crate::pl::{self, arg, cap, n, ret};

/// `Image::ExifTool::NAME(ARGS)` (the functions imported with :DataAccess :Utils).
fn call(it: &mut Interp, name: &str, args: Vec<Value>) -> ER<Value> {
    pl::call(it, &format!("Image::ExifTool::{name}"), args)
}

/// `{ Name => $name }`
fn name_hash(name: Value) -> Value {
    let h = Value::new_hash();
    h.set("Name", name);
    h
}

fn handle_struct_impl(it: &mut Interp, et: &Value, struct_: &Value, pid: &Value, pname: &Value, lang: &Value, ctry: &Value) -> ER<()> {
    let (mut lang, mut ctry) = (lang.clone(), ctry.clone());
    let tag_tbl = call(it, "GetTagTable", vec![pl::s("Image::ExifTool::Matroska::StdTag")])?;
    let tag = struct_.get("TagName");
    let mut tag_info = tag_tbl.get(tag.to_bytes());
    // create tag if necessary
    if pl::ref_of(&tag_info) != "HASH" {
        let mut name = pl::ucfirst(&pl::lc(&tag));
        pl::tr(&mut name, "0-9a-zA-Z_", "", "dc");
        pl::re("_([a-z])", "").subst(&mut name, true, |c| cap(c, 1).to_bytes().to_ascii_uppercase());
        if pl::lt(&pl::length(&name), &n(2)) {
            name = pl::concat(&pl::s("Tag_"), &name);
        }
        let msg = pl::concat(&pl::concat(&pl::concat(&pl::concat(&pl::s("  [adding "), &tag), &pl::s(" = ")), &name), &pl::s("]\n"));
        pl::method(it, et, "VPrint", vec![n(0), msg])?;
        tag_info = call(it, "AddTagToTable", vec![tag_tbl.clone(), tag.clone(), name_hash(name)])?;
    }
    let (mut id, mut nm);
    if pid.truthy() {
        id = pl::concat(&pl::concat(pid, &pl::s("/")), &tag);
        nm = pl::concat(&pl::concat(pname, &pl::s("/")), &tag_info.get("Name"));
        if !tag_tbl.get(id.to_bytes()).truthy() {
            let copy = tag_info.hash_copy();
            copy.set("Name", nm.clone());
            let msg = pl::concat(&pl::concat(&pl::concat(&pl::concat(&pl::s("  [adding "), &id), &pl::s(" = ")), &nm), &pl::s("]\n"));
            pl::method(it, et, "VPrint", vec![n(0), msg])?;
            tag_info = call(it, "AddTagToTable", vec![tag_tbl.clone(), id.clone(), copy])?;
        }
    } else {
        id = tag.clone();
        nm = tag_info.get("Name");
    }
    if !struct_.get("TagString").is_undef() || !struct_.get("TagBinary").is_undef() {
        let ts = struct_.get("TagString");
        let val = if !ts.is_undef() { ts } else { pl::elem_ref(struct_, b"TagBinary") };
        let l = struct_.get("TagLanguageBCP47");
        lang = if l.truthy() {
            l
        } else {
            let l = struct_.get("TagLanguage");
            if l.truthy() {
                l
            } else {
                lang
            }
        };
        // (Note: not currently handling TagDefault attribute)
        let mut code = lang.clone();
        if ctry.truthy() {
            code = if lang.truthy() { pl::concat(&pl::concat(&lang, &pl::s("-")), &ctry) } else { pl::concat(&pl::s("eng-"), &ctry) }; // ('eng' is default lang)
        }
        if code.truthy() {
            tag_info = pl::call(it, "Image::ExifTool::GetLangInfo", vec![tag_info.clone(), code])?;
            pl::method(it, et, "HandleTag", vec![tag_tbl.clone(), tag_info.get("TagID"), val.clone()])?;
        } else {
            pl::method(it, et, "HandleTag", vec![tag_tbl.clone(), id.clone(), val.clone()])?;
        }
        // COUNTRY is handled as an attribute for contained tags
        if tag.eq_str("COUNTRY") {
            ctry = val;
            id = pid.clone();
            nm = pname.clone();
        }
    }
    let st = struct_.get("struct");
    if st.truthy() {
        // step into each contained structure
        for s in st.items() {
            handle_struct_impl(it, et, &s, &id, &nm, &lang, &ctry)?;
        }
    }
    Ok(())
}

/// Handle MKV SimpleTag structure
/// Inputs: 0) ExifTool ref, 1) structure ref, 2) parent tag ID, 3) parent tag Name,
///         4) language code, 5) country code
pub fn handle_struct(it: &mut Interp, a: Vec<Value>, want: Want) -> ER<Vec<Value>> {
    pl::local_topic(it, |it| {
        handle_struct_impl(it, &arg(&a, 0), &arg(&a, 1), &arg(&a, 2), &arg(&a, 3), &arg(&a, 4), &arg(&a, 5))?;
        // (the value of the last statement, an "if" without else)
        pl::ret_nothing(want)
    })
}

/// `ord(substr($_[0], $_[1]++))`
fn next_byte(data: &[u8], pos: &mut i64) -> i64 {
    let b = data.get(*pos as usize).map_or(0, |b| *b as i64);
    *pos += 1;
    b
}

fn get_vint_impl(data: &[u8], pos: &mut i64) -> Value {
    let len = data.len() as i64;
    if *pos >= len {
        return Value::Undef;
    }
    let mut val = next_byte(data, pos);
    let mut num = 0;
    if val == 0 {
        if *pos >= len {
            return Value::Undef;
        }
        val = next_byte(data, pos);
        if val == 0 {
            return Value::Undef; // can't be this large!
        }
        num += 7; // 7 more bytes to read (we just read one)
    }
    let mut mask = 0x7f;
    while val == (val & mask) {
        mask >>= 1;
        num += 1;
    }
    val &= mask;
    let mut unknown = val == mask;
    if *pos + num > len {
        return Value::Undef;
    }
    let mut v = n(val);
    while num != 0 {
        let b = next_byte(data, pos);
        if b != 0xff {
            unknown = false;
        }
        v = pl::add(&pl::mul(&v, &n(256)), &n(b));
        num -= 1;
    }
    if unknown {
        n(-1)
    } else {
        v
    }
}

/// `GetVInt($buff, $pos)` on a Perl position value.
fn get_vint_v(data: &Value, pos: &mut Value) -> Value {
    let mut p = pos.to_i64();
    let r = get_vint_impl(&data.to_bytes(), &mut p);
    *pos = n(p);
    r
}

/// Get variable-length Matroska integer
/// Inputs: 0) data buffer, 1) position in data
/// Returns: integer value and updates position, -1 for unknown/reserved value,
///          or undef if no data left
/// Notes: Increments position pointer
pub fn get_vint(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let data = arg(&a, 0);
    let mut pos = arg(&a, 1);
    let start = pos.clone();
    let r = get_vint_v(&data, &mut pos);
    // ($_[1] is written only when it was incremented)
    if !pl::num_eq(&pos, &start) {
        it.set_arg(1, pos)?;
    }
    ret(r)
}

/// One open master element: its end, its name and the structure it was in.
struct DirEnd {
    end: Value,
    name: Value,
    struct_: Value,
}

/// Read information from a Matroska multimedia file (MKV, MKA, MKS)
/// Inputs: 0) ExifTool object reference, 1) Directory information reference
/// Returns: 1 on success, 0 if this wasn't a valid Matroska file
pub fn process_mkv(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info) = (arg(&a, 0), arg(&a, 1));
    let raf = dir_info.get("RAF");
    let buff = cell(Value::Undef);
    let buf2 = cell(Value::Undef);
    let buff_ref = Value::ScalarRef(buff.clone());
    let b = || buff.borrow().clone();
    let mut dir_end: Vec<DirEnd> = Vec::new();
    let mut track_indent = Value::Undef;
    let track_types = Value::new_hash();
    let track_num = Value::new_hash();
    let mut struct_ = Value::Undef;
    let mut seek_info = Value::new_hash();
    let seek = Value::new_hash();

    if !pl::num_eq(&raf_read_cell(it, &raf, &buff, n(4))?, &n(4)) {
        return ret(n(0));
    }
    if !pl::re(r"^\x1a\x45\xdf\xa3", "").is_match(&b()) {
        return ret(n(0));
    }

    // read in 64kB blocks (already read 4 bytes)
    if !raf_read_cell(it, &raf, &buff, n(65532))?.truthy() {
        return ret(n(0));
    }
    let mut data_len = pl::length(&b());
    let (mut pos, mut data_pos) = (n(0), n(4));

    // verify header length
    let hlen = get_vint_v(&b(), &mut pos);
    if !(hlen.truthy() && pl::gt(&hlen, &n(0))) {
        return ret(n(0));
    }
    if pl::gt(&pl::add(&pos, &hlen), &data_len) {
        pl::method(it, &et, "Warn", vec![pl::s("Truncated Matroska header")])?;
        return ret(n(1));
    }
    pl::method(it, &et, "SetFileType", vec![])?;
    call(it, "SetByteOrder", vec![pl::s("MM")])?;
    let tag_table_ptr = call(it, "GetTagTable", vec![pl::s("Image::ExifTool::Matroska::Main")])?;

    // set flag to process entire file (otherwise we stop at the first Cluster)
    let verbose = pl::method(it, &et, "Options", vec![pl::s("Verbose")])?;
    let mut process_all = if verbose.truthy() || pl::gt(&pl::method(it, &et, "Options", vec![pl::s("Unknown")])?, &n(1)) { 2 } else { 0 };
    if pl::method(it, &et, "Options", vec![pl::s("ExtractEmbedded")])?.truthy() {
        process_all += 1;
    }
    et.set("TrackTypes", track_types.clone()); // store Track types reference
    et.set("SeekHeadOffset", n(0));
    let old_indent = et.get("INDENT");
    let mut chapter_num = n(0);
    let mut dir_name = pl::s("MKV");
    let indent_minus2 = |it: &mut Interp, et: &Value| -> ER<Value> { substr(it, &et.get("INDENT"), &n(0), Some(&n(-2))) };

    // loop over all Matroska elements
    loop {
        while let Some(last) = dir_end.last() {
            if pl::ge(&pl::add(&pos, &data_pos), &last.end) {
                if last.name.eq_str("Seek") {
                    // save seek info
                    if !seek_info.get("ID").is_undef() && !seek_info.get("Position").is_undef() {
                        let seek_tag = tag_table_ptr.get(seek_info.get("ID").to_bytes());
                        if pl::ref_of(&seek_tag) == "HASH" && seek_tag.get("Name").truthy() {
                            seek.set(seek_tag.get("Name").to_bytes(), pl::add(&seek_info.get("Position"), &et.get("SeekHeadOffset")));
                        }
                    }
                    seek_info = Value::new_hash();
                }
                dir_end.pop();
                if struct_.truthy() {
                    if let Some(l) = dir_end.last().filter(|l| l.struct_.truthy()) {
                        // save this nested structure
                        l.struct_.get_or_new_array("struct").push(struct_.clone());
                        struct_ = l.struct_.clone();
                    } else {
                        // handle completed structures now
                        handle_struct_impl(it, &et, &struct_, &Value::Undef, &Value::Undef, &Value::Undef, &Value::Undef)?;
                        struct_ = Value::Undef;
                    }
                }
                dir_name = match dir_end.last() {
                    Some(l) => l.name.clone(),
                    None => pl::s("MKV"),
                };
                // use INDENT to decide whether or not we are done this Track element
                if track_indent.truthy() && track_indent.to_bytes() == et.get("INDENT").to_bytes() {
                    et.delete("SET_GROUP1");
                }
                let ind = indent_minus2(it, &et)?;
                et.set("INDENT", ind);
                if let Value::ArrayRef(p) = et.get("PATH") {
                    p.borrow_mut().pop();
                }
            } else {
                dir_name = last.name.clone();
                break;
            }
        }
        let _ = &dir_name;
        // read more if we are getting close to the end of our buffer
        // (24 more bytes should be enough to read this element header)
        if pl::gt(&pl::add(&pos, &n(24)), &data_len) && raf_read_cell(it, &raf, &buf2, n(65536))?.truthy() {
            let nb = pl::concat(&substr(it, &b(), &pos, None)?, &buf2.borrow().clone());
            *buff.borrow_mut() = nb;
            *buf2.borrow_mut() = Value::Undef;
            data_pos = pl::add(&data_pos, &pos);
            data_len = pl::length(&b());
            pos = n(0);
        }
        let tag = get_vint_v(&b(), &mut pos);
        if !(!tag.is_undef() && pl::ge(&tag, &n(0))) {
            break;
        }
        if pl::num_eq(&tag, &n(0x14d9b74)) {
            et.set("SeekHeadOffset", pos.clone()); // save offset of seek head
        }
        let mut size = get_vint_v(&b(), &mut pos);
        if size.is_undef() {
            break;
        }
        let (mut unknown_size, mut seek_info_only) = (false, false);
        let mut tag_name = Value::Undef;
        if pl::lt(&size, &n(0)) {
            unknown_size = true;
            size = Value::Num(1e20);
        }
        if let Some(last) = dir_end.last() {
            if pl::gt(&pl::add(&pl::add(&pos, &data_pos), &size), &last.end) {
                let msg = pl::concat(&pl::concat(&pl::s("Invalid or corrupted "), &last.name), &pl::s(" master element"));
                pl::method(it, &et, "Warn", vec![msg])?;
                pos = pl::sub(&last.end, &data_pos);
                if pl::lt(&pos, &n(0)) || pl::gt(&pos, &data_len) {
                    *buff.borrow_mut() = pl::s("");
                    data_pos = pl::add(&data_pos, &pos);
                    data_len = n(0);
                    pos = n(0);
                    if !pl::method(it, &raf, "Seek", vec![data_pos.clone(), n(0)])?.truthy() {
                        break;
                    }
                }
                continue;
            }
        }
        let mut tag_info = pl::method(it, &et, "GetTagInfo", vec![tag_table_ptr.clone(), tag.clone()])?;
        let te = tag_table_ptr.get(tag.to_bytes());
        if !tag_info.truthy() && pl::ref_of(&te) == "HASH" && te.get("SeekInfo").truthy() {
            tag_info = te;
            seek_info_only = true;
        }
        if tag_info.truthy() {
            tag_name = tag_info.get("Name");
            if tag_info.get("SubDirectory").truthy() && !tag_info.get("NotEBML").truthy() {
                // stop processing at first cluster unless we are using -v -U or -ee
                // or there are Tags after this
                if tag_name.eq_str("Cluster") && process_all < 2 {
                    // jump to Tags if possible
                    if process_all == 0 {
                        let st = seek.get("Tags");
                        if st.truthy() && pl::gt(&st, &pl::add(&pos, &data_pos)) && pl::method(it, &raf, "Seek", vec![st.clone(), n(0)])?.truthy() {
                            *buff.borrow_mut() = pl::s("");
                            data_pos = st;
                            pos = n(0);
                            data_len = n(0);
                            continue;
                        }
                        break;
                    }
                    tag_info = Value::Undef; // just skip the Cluster when -ee is used
                } else {
                    // just fall through into the contained EBML elements
                    et.set("INDENT", pl::concat(&et.get("INDENT"), &pl::s("| ")));
                    dir_name = tag_name.clone();
                    pl::method(it, &et, "VerboseDir", vec![dir_name.clone(), Value::Undef, size.clone()])?;
                    et.get_or_new_array("PATH").push(dir_name.clone());
                    dir_end.push(DirEnd { end: pl::add(&pl::add(&pos, &data_pos), &size), name: dir_name.clone(), struct_: struct_.clone() });
                    if dir_name.eq_str("SimpleTag") {
                        struct_ = Value::new_hash(); // keep track of SimpleTag elements
                    }
                    // set Chapter# and Info family 1 group names
                    if tag_name.eq_str("ChapterAtom") {
                        chapter_num = pl::add(&chapter_num, &n(1));
                        et.set("SET_GROUP1", pl::concat(&pl::s("Chapter"), &chapter_num));
                        track_indent = et.get("INDENT");
                    } else if tag_name.eq_str("Info") && !et.get("SET_GROUP1").truthy() {
                        et.set("SET_GROUP1", pl::s("Info"));
                        track_indent = et.get("INDENT");
                    }
                    continue;
                }
            }
        } else if verbose.truthy() {
            let fmt = pl::concat(&et.get("INDENT"), &pl::s("- Tag 0x%x (Unknown, %d bytes)\n"));
            let msg = pl::sprintf_v(&fmt, &[tag.clone(), size.clone()]);
            pl::method(it, &et, "VPrint", vec![n(0), msg])?;
        }
        if unknown_size {
            break;
        }
        if pl::gt(&pl::add(&pos, &size), &data_len) {
            // how much more do we need to read?
            let mut more = pl::sub(&pl::add(&pos, &size), &data_len);
            // just skip unknown and large data blocks
            if !tag_info.truthy() || pl::gt(&more, &n(10000000)) {
                // don't try to skip very large blocks unless LargeFileSupport is enabled
                if pl::ge(&more, &n(0x80000000)) {
                    if !pl::method(it, &et, "Options", vec![pl::s("LargeFileSupport")])?.truthy() {
                        break;
                    }
                    if pl::method(it, &et, "Options", vec![pl::s("LargeFileSupport")])?.eq_str("2") {
                        pl::method(it, &et, "Warn", vec![pl::s("Processing large block (LargeFileSupport is 2)")])?;
                    }
                }
                if !pl::method(it, &raf, "Seek", vec![more.clone(), n(1)])?.truthy() {
                    break;
                }
                *buff.borrow_mut() = pl::s("");
                data_pos = pl::add(&data_pos, &pl::add(&data_len, &more));
                data_len = n(0);
                pos = n(0);
                continue;
            } else {
                // read data in multiples of 64kB
                more = pl::mul(&pl::add(&pl::int(&pl::div(&more, &n(65536))?), &n(1)), &n(65536));
                if raf_read_cell(it, &raf, &buf2, more.clone())?.truthy() {
                    let nb = pl::concat(&substr(it, &b(), &pos, None)?, &buf2.borrow().clone());
                    *buff.borrow_mut() = nb;
                    *buf2.borrow_mut() = Value::Undef;
                    data_pos = pl::add(&data_pos, &pos);
                    data_len = pl::length(&b());
                    pos = n(0);
                }
                if pl::gt(&pl::add(&pos, &size), &data_len) {
                    break;
                }
            }
        }
        if !tag_info.truthy() {
            // ignore the element
            pos = pl::add(&pos, &size);
            continue;
        }
        let mut val = Value::Undef;
        if tag_info.get("Format").truthy() {
            let fmt = tag_info.get("Format");
            if fmt.eq_str("string") || fmt.eq_str("utf8") {
                val = substr(it, &b(), &pos, Some(&size))?;
                pl::re(r"\0.*", "s").subst(&mut val, false, |_| Vec::new());
                if fmt.eq_str("utf8") {
                    val = pl::method(it, &et, "Decode", vec![val, pl::s("UTF8")])?;
                }
            } else if fmt.eq_str("float") {
                if pl::num_eq(&size, &n(4)) {
                    val = call(it, "GetFloat", vec![buff_ref.clone(), pos.clone()])?;
                } else if pl::num_eq(&size, &n(8)) {
                    val = call(it, "GetDouble", vec![buff_ref.clone(), pos.clone()])?;
                } else {
                    let msg = pl::concat(&pl::concat(&pl::s("Illegal float size ("), &size), &pl::s(")"));
                    pl::method(it, &et, "Warn", vec![msg])?;
                }
            } else {
                let vals = pl::unpack(&format!("x{}C{}", pos.to_text(), size.to_text()), &b())?;
                val = n(0);
                if fmt.eq_str("signed") || fmt.eq_str("date") {
                    let mut over = n(1);
                    for v in &vals {
                        val = pl::add(&pl::mul(&val, &n(256)), v);
                        over = pl::mul(&over, &n(256));
                    }
                    // interpret negative numbers
                    if arg(&vals, 0).to_i64() & 0x80 != 0 {
                        val = pl::sub(&val, &over);
                    }
                    // convert dates (nanoseconds since 2001:01:01)
                    if fmt.eq_str("date") {
                        let mut t = pl::div(&val, &Value::Num(1e9))?;
                        // (8 leap days between 1970 and 2001)
                        t = pl::add(&t, &n(((2001 - 1970) * 365 + 8) * 24 * 3600));
                        let u = pl::call(it, "Image::ExifTool::ConvertUnixTime", vec![t, Value::Undef, n(-9)])?;
                        val = pl::concat(&u, &pl::s("Z"));
                    }
                } else {
                    // must be unsigned
                    for v in &vals {
                        val = pl::add(&pl::mul(&val, &n(256)), v);
                    }
                }
            }
            if tag_name.eq_str("TrackNumber") {
                // set Track# family 1 group name for tags directly in the track
                et.set("SET_GROUP1", pl::concat(&pl::s("Track"), &val));
                track_indent = et.get("INDENT");
            } else if tag_name.eq_str("TrackUID") && et.get("SET_GROUP1").truthy() {
                // save the Track# group associated with this TrackUID
                track_num.set(val.to_bytes(), et.get("SET_GROUP1"));
            } else if tag_name.eq_str("TagTrackUID") && track_num.get(val.to_bytes()).truthy() {
                // set Track# group for associated SimpleTags tags
                et.set("SET_GROUP1", track_num.get(val.to_bytes()));
                // we're already one deeper than the level where we want to
                // reset the group name, so trigger at one indent level higher
                track_indent = indent_minus2(it, &et)?;
            }
        }
        let parms = vec![pl::s("DataPt"), buff_ref.clone(), pl::s("DataPos"), data_pos.clone(), pl::s("Start"), pos.clone(), pl::s("Size"), size.clone()];
        if tag_info.get("NoSave").truthy() || struct_.truthy() {
            if verbose.truthy() {
                let mut args = vec![tag.clone(), tag_info.clone(), pl::s("Value"), val.clone()];
                args.extend(parms);
                pl::method(it, &et, "VerboseInfo", args)?;
            }
            if struct_.truthy() {
                struct_.set(tag_name.to_bytes(), val);
            }
        } else if tag_info.get("SeekInfo").truthy() {
            let mut p = pos.clone();
            if val.is_undef() {
                val = get_vint_v(&b(), &mut p);
            }
            seek_info.set(tag_info.get("SeekInfo").to_bytes(), val.clone());
            if !seek_info_only {
                let mut args = vec![tag_table_ptr.clone(), tag.clone(), val];
                args.extend(parms);
                pl::method(it, &et, "HandleTag", args)?;
            }
        } else {
            let mut args = vec![tag_table_ptr.clone(), tag.clone(), val];
            args.extend(parms);
            pl::method(it, &et, "HandleTag", args)?;
        }
        pos = pl::add(&pos, &size); // step to next element
    }
    et.set("INDENT", old_indent);
    et.delete("SET_GROUP1");
    // override file type if necessary based on existing track types
    if !(track_types.get("1").truthy() || track_types.get("3").truthy()) {
        // video or complex?
        if track_types.get("2").truthy() {
            // audio?
            pl::method(it, &et, "OverrideFileType", vec![pl::s("MKA")])?;
        } else if track_types.get("17").truthy() {
            // subtitle?
            pl::method(it, &et, "OverrideFileType", vec![pl::s("MKS")])?;
        }
    }
    ret(n(1))
}

pub fn register(it: &mut Interp) {
    it.define_native("Image::ExifTool::Matroska::HandleStruct", handle_struct);
    it.define_native("Image::ExifTool::Matroska::GetVInt", get_vint);
    it.define_native("Image::ExifTool::Matroska::ProcessMKV", process_mkv);
}
