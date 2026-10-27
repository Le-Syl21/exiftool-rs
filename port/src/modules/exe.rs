//! Image::ExifTool::EXE (Image/ExifTool/EXE.pm), ported sub by sub (see docs/port/PORTING.md).
//!
//! Read meta information of various executable file formats

// (the Perl assigns values it does not read again; the port keeps them)
#![allow(unused_assignments)]

use crate::interp::eval::{Cell, ER};
use crate::interp::{Interp, Want};
use crate::modules::random_access::{cell, raf_read_cell, substr};
use crate::perl::Value;
use crate::pl::{self, arg, cap, n, re, ret};
use crate::runtime;

const MODULE: &str = "Image::ExifTool::EXE";

/// `Image::ExifTool::NAME(ARGS)` (the functions imported with :DataAccess :Utils).
fn call(it: &mut Interp, name: &str, args: Vec<Value>) -> ER<Value> {
    pl::call(it, &format!("Image::ExifTool::{name}"), args)
}

/// `\$buff` of a `my` variable kept in a cell.
fn rf(c: &Cell) -> Value {
    Value::ScalarRef(c.clone())
}

fn val(c: &Cell) -> Value {
    c.borrow().clone()
}

fn set(c: &Cell, v: Value) {
    *c.borrow_mut() = v;
}

/// `$raf->Read($buff, $len)` into the `my $buff` cell.
fn read(it: &mut Interp, raf: &Value, buff: &Cell, len: Value) -> ER<Value> {
    raf_read_cell(it, raf, buff, len)
}

fn seek(it: &mut Interp, raf: &Value, pos: Value, whence: i64) -> ER<bool> {
    Ok(pl::method(it, raf, "Seek", vec![pos, n(whence)])?.truthy())
}

fn warn(it: &mut Interp, et: &Value, msg: &str) -> ER<()> {
    pl::method(it, et, "Warn", vec![pl::s(msg)])?;
    Ok(())
}

fn cat(parts: &[&Value]) -> Value {
    let mut v = Vec::new();
    for p in parts {
        v.extend_from_slice(&p.to_bytes());
    }
    Value::bytes(v)
}

/// `$a & $b` on perl numbers (as unsigned integers).
fn band(a: &Value, b: i64) -> Value {
    n(((a.to_i64() as u64) & (b as u64)) as i64)
}

fn handle_tag(it: &mut Interp, et: &Value, table: &Value, tag: Value, v: Value, opts: &[(&str, Value)]) -> ER<Value> {
    let mut args = vec![table.clone(), tag, v];
    for (k, x) in opts {
        args.push(pl::s(k));
        args.push(x.clone());
    }
    pl::method(it, et, "HandleTag", args)
}

fn hash(pairs: &[(&str, Value)]) -> Value {
    let h = Value::new_hash();
    for (k, v) in pairs {
        h.set(k, v.clone());
    }
    h
}

/// `$raf->METHOD(...)` dies as perl does on an undefined `$raf` (the read
/// itself runs as a Perl snippet, whose messages name the snippet).
fn raf_defined(raf: &Value, method: &str) -> ER<()> {
    if raf.is_undef() {
        return crate::interp::eval::die(format!("Can't call method \"{method}\" on an undefined value"));
    }
    Ok(())
}

/// Extract information from a CHM file
/// Inputs: 0) ExifTool object reference, 1) dirInfo reference
/// Returns: 1 on success, 0 if this wasn't a valid CHM file
pub fn process_chm(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info) = (arg(&a, 0), arg(&a, 1));
    let raf = dir_info.get("RAF");
    let buff = cell(Value::Undef);

    pl::at(918);
    raf_defined(&raf, "Read")?;
    if !(pl::num_eq(&read(it, &raf, &buff, n(56))?, &n(56))
        && re(r"^ITSF.{20}\x10\xfd\x01\x7c\xaa\x7b\xd0\x11\x9e\x0c\0\xa0\xc9\x22\xe6\xec", "s").is_match(&val(&buff)))
    {
        return ret(n(0));
    }
    let tag_table_ptr = call(it, "GetTagTable", vec![pl::s("Image::ExifTool::EXE::CHM")])?;
    pl::method(it, &et, "SetFileType", vec![])?;
    call(it, "SetByteOrder", vec![pl::s("II")])?;
    pl::method(it, &et, "ProcessDirectory", vec![hash(&[("DataPt", rf(&buff))]), tag_table_ptr])?;
    ret(n(1))
}

fn read_unicode_str_impl(it: &mut Interp, data_pt: &Value, mut pos: Value, mut end: Value, et: &Value) -> ER<(Value, Value)> {
    pl::at(935);
    let data = pl::sderef(data_pt)?;
    if pl::gt(&end, &pl::length(&data)) {
        end = pl::length(&data); // (be safe)
    }
    let mut str_ = Vec::new();
    while pl::le(&pl::add(&pos, &n(2)), &end) {
        let ch = substr(it, &data, &pos, Some(&n(2)))?;
        pos = pl::add(&pos, &n(2));
        if pl::str_eq(&ch, "\0\0") {
            break;
        }
        str_.extend_from_slice(&ch.to_bytes());
    }
    if band(&pos, 0x03).truthy() {
        pos = pl::add(&pos, &n(2));
    }
    let to = if et.truthy() { pl::method(it, et, "Options", vec![pl::s("Charset")])? } else { pl::s("UTF8") };
    let s = call(it, "Decode", vec![Value::Undef, Value::bytes(str_), pl::s("UTF16"), pl::s("II"), to])?;
    Ok((s, pos))
}

/// Read Unicode string (null terminated) from resource
/// Inputs: 0) data ref, 1) start offset, 2) data end, 3) optional ExifTool object ref
/// Returns: 0) Unicode string translated to UTF8, or current CharSet with ExifTool ref
///          1) end pos (rounded up to nearest 4 bytes)
pub fn read_unicode_str(it: &mut Interp, a: Vec<Value>, want: Want) -> ER<Vec<Value>> {
    let (s, pos) = read_unicode_str_impl(it, &arg(&a, 0), arg(&a, 1), arg(&a, 2), &arg(&a, 3))?;
    // (a list in scalar context gives its last element)
    if want == Want::List {
        Ok(vec![s, pos])
    } else {
        ret(pos)
    }
}

fn process_pe_version_impl(it: &mut Interp, et: &Value, dir_info: &Value) -> ER<Value> {
    let data_pt = dir_info.get("DataPt");
    let mut pos = dir_info.get("DirStart");
    let end = pl::add(&pos, &dir_info.get("DirLen"));
    let (mut index, mut len, mut val_len, mut string, mut str_end);

    // get VS_VERSION_INFO
    index = n(0);
    loop {
        pos = band(&pl::add(&pos, &n(3)), 0xfffffffc); // align on a 4-byte boundary
        if pl::gt(&pl::add(&pos, &n(6)), &end) {
            break;
        }
        len = call(it, "Get16u", vec![data_pt.clone(), pos.clone()])?;
        if pl::gt(&pl::add(&pos, &len), &end) {
            return Ok(n(0));
        }
        val_len = call(it, "Get16u", vec![data_pt.clone(), pl::add(&pos, &n(2))])?;
        let _type = call(it, "Get16u", vec![data_pt.clone(), pl::add(&pos, &n(4))])?;
        if !(len.truthy() || val_len.truthy()) {
            return Ok(n(0)); // prevent possible infinite loop
        }
        (string, str_end) = read_unicode_str_impl(it, &data_pt, pl::add(&pos, &n(6)), pl::add(&pos, &len), &Value::Undef)?;
        if pl::gt(&pl::add(&str_end, &val_len), &end) {
            return Ok(n(0));
        }
        if !(index.truthy() || pl::str_eq(&string, "VS_VERSION_INFO")) {
            warn(it, et, "Invalid Version Info block")?;
            return Ok(n(0));
        }
        if pl::str_eq(&string, "VS_VERSION_INFO") {
            // parse the fixed version info
            dir_info.set("DirStart", str_end.clone());
            dir_info.set("DirLen", val_len.clone());
            let sub_table_ptr = call(it, "GetTagTable", vec![pl::s("Image::ExifTool::EXE::PEVersion")])?;
            pl::method(it, et, "ProcessDirectory", vec![dir_info.clone(), sub_table_ptr])?;
            pos = pl::add(&str_end, &val_len);
        } else if pl::str_eq(&string, "StringFileInfo") && pl::num_eq(&val_len, &n(0)) {
            pos = pl::add(&pos, &len);
            let mut pt = str_end.clone();
            // parse string table
            let tag_table_ptr = call(it, "GetTagTable", vec![pl::s("Image::ExifTool::EXE::PEString")])?;
            index = n(0);
            'entries: while pl::lt(&pl::add(&pt, &n(6)), &pos) {
                'body: {
                    len = call(it, "Get16u", vec![data_pt.clone(), pt.clone()])?;
                    if !len.truthy() {
                        warn(it, et, "Invalid PEString entry")?;
                        break 'entries;
                    }
                    val_len = call(it, "Get16u", vec![data_pt.clone(), pl::add(&pt, &n(2))])?;
                    // $type = Get16u($dataPt, $pt + 4);
                    let entry_end = pl::add(&pt, &len);
                    if pl::gt(&entry_end, &end) {
                        return Ok(n(0));
                    }
                    // get tag ID (converted to UTF8)
                    (string, pt) = read_unicode_str_impl(it, &data_pt, pl::add(&pt, &n(6)), entry_end.clone(), &Value::Undef)?;
                    if !index.truthy() {
                        // separate the language code and character set
                        // (not sure what the CharacterSet tag is for, but the string
                        // values stored here are UCS-2 in all my files even if the
                        // CharacterSet is otherwise)
                        let mut char_ = Value::Undef;
                        if pl::gt(&pl::length(&string), &n(4)) {
                            char_ = pl::substr(&string, 4, None);
                            string = pl::substr(&string, 0, Some(4));
                        }
                        handle_tag(it, et, &tag_table_ptr, pl::s("LanguageCode"), pl::uc(&string), &[])?;
                        if char_.truthy() {
                            handle_tag(it, et, &tag_table_ptr, pl::s("CharacterSet"), pl::uc(&char_), &[])?;
                        }
                        break 'body; // next
                    }
                    let tag = string.clone();
                    // get tag value (converted to current Charset)
                    if val_len.truthy() {
                        (string, pt) = read_unicode_str_impl(it, &data_pt, pt.clone(), entry_end.clone(), et)?;
                    } else {
                        string = pl::s("");
                    }
                    handle_tag(it, et, &tag_table_ptr, tag, string.clone(), &[("MakeTagInfo", n(1))])?;
                    // step to next entry (padded to an even word)
                    pt = band(&pl::add(&entry_end, &n(3)), 0xfffffffc);
                }
                index = pl::add(&index, &n(1));
            }
        } else {
            pos = pl::add(&pos, &pl::add(&len, &val_len));
            // ignore other information (for now)
        }
        index = pl::add(&index, &n(1));
    }
    Ok(n(1))
}

/// Process Windows PE Version Resource
/// Inputs: 0) ExifTool object ref, 1) dirInfo ref
/// Returns: true on success
pub fn process_pe_version(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    ret(process_pe_version_impl(it, &arg(&a, 0), &arg(&a, 1))?)
}

fn get_file_offset_impl(addr: &Value, sections: &Value) -> Value {
    for section in sections.items() {
        let va = section.get("VirtualAddress");
        if !(pl::ge(addr, &va) && pl::lt(addr, &pl::add(&va, &section.get("Size")))) {
            continue;
        }
        return pl::sub(&pl::add(addr, &section.get("Base")), &va);
    }
    Value::Undef
}

/// Get actual file offset given a virtual address in a PE file
/// Inputs: 0) virtual address, 1) section list ref
/// Returns: absolute file offset or undef on error
pub fn get_file_offset(_it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    ret(get_file_offset_impl(&arg(&a, 0), &arg(&a, 1)))
}

fn process_pe_resources_impl(it: &mut Interp, et: &Value, dir_info: &Value) -> ER<Value> {
    let raf = dir_info.get("RAF");
    let base = dir_info.get("Base");
    let dir_start = pl::add(&dir_info.get("DirStart"), &base);
    let lv = dir_info.get("Level");
    let level = if lv.truthy() { lv } else { n(0) };
    let verbose = pl::method(it, et, "Options", vec![pl::s("Verbose")])?;
    let buff = cell(Value::Undef);

    if pl::gt(&level, &n(10)) {
        return Ok(n(0)); // protect against deep recursion
    }
    // read the resource header
    pl::at(1061);
    raf_defined(&raf, "Seek")?;
    if !(seek(it, &raf, dir_start, 0)? && pl::num_eq(&read(it, &raf, &buff, n(16))?, &n(16))) {
        return Ok(n(0));
    }
    let name_entries = call(it, "Get16u", vec![rf(&buff), n(12)])?;
    let id_entries = call(it, "Get16u", vec![rf(&buff), n(14)])?;
    let count = pl::add(&name_entries, &id_entries);
    if pl::gt(&count, &n(10000)) {
        return Ok(n(0));
    }
    let want = pl::mul(&count, &n(8));
    if !pl::num_eq(&read(it, &raf, &buff, want.clone())?, &want) {
        return Ok(n(0));
    }
    let resource_type = runtime::lex_hash(it, MODULE, "resourceType");
    // loop through all resource entries
    let mut item = n(0);
    while pl::lt(&item, &count) {
        'body: {
            let pos = pl::mul(&item, &n(8));
            let name = call(it, "Get32u", vec![rf(&buff), pos.clone()])?;
            let entry_pos = call(it, "Get32u", vec![rf(&buff), pl::add(&pos, &n(4))])?;
            if !level.truthy() {
                // set resource type if this is the 0th level directory
                let rt = resource_type.get(name.to_bytes());
                let res_type = if rt.truthy() { rt } else { pl::sprintf("Unknown (0x%x)", &[name.clone()]) };
                // ignore everything but the Version resource unless verbose
                if verbose.truthy() {
                    pl::method(it, et, "VPrint", vec![n(0), cat(&[&res_type, &pl::s(" resource:\n")])])?;
                } else if !pl::str_eq(&res_type, "Version") {
                    break 'body;
                }
                dir_info.set("ResType", res_type);
            }
            if band(&entry_pos, 0x80000000).truthy() {
                // is this a directory?
                // descend into next directory level
                dir_info.set("DirStart", band(&entry_pos, 0x7fffffff));
                dir_info.set("Level", pl::add(&level, &n(1)));
                if !process_pe_resources_impl(it, et, dir_info)?.truthy() {
                    return Ok(n(0));
                }
                dir_info.set("Level", pl::sub(&dir_info.get("Level"), &n(1)));
            } else if dir_info.get("ResType").eq_str("Version") && pl::num_eq(&level, &n(2)) && !dir_info.get("GotVersion").truthy() {
                // (only process first Version resource)
                // get position of this resource in the file
                let buf2 = cell(Value::Undef);
                if !(seek(it, &raf, pl::add(&entry_pos, &base), 0)? && pl::num_eq(&read(it, &raf, &buf2, n(16))?, &n(16))) {
                    return Ok(n(0));
                }
                let addr = call(it, "Get32u", vec![rf(&buf2), n(0)])?;
                let len = call(it, "Get32u", vec![rf(&buf2), n(4)])?;
                // determine which section this is in so we can convert the virtual address
                let file_off = get_file_offset_impl(&addr, &dir_info.get("Sections"));
                if !file_off.truthy() {
                    return Ok(n(0));
                }
                if !(seek(it, &raf, file_off, 0)? && pl::num_eq(&read(it, &raf, &buf2, len.clone())?, &len)) {
                    return Ok(n(0));
                }
                let d = hash(&[("DataPt", rf(&buf2)), ("DataLen", len.clone()), ("DirStart", n(0)), ("DirLen", len.clone())]);
                if !process_pe_version_impl(it, et, &d)?.truthy() {
                    warn(it, et, "Possibly corrupt Version resource")?;
                }
                dir_info.set("GotVersion", n(1)); // set flag so we don't do this again
            }
        }
        item = pl::add(&item, &n(1));
    }
    Ok(n(1))
}

/// Process Windows PE Resources
/// Inputs: 0) ExifTool object ref, 1) dirInfo ref
/// Returns: true on success
pub fn process_pe_resources(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    ret(process_pe_resources_impl(it, &arg(&a, 0), &arg(&a, 1))?)
}

fn process_pe_dict_impl(it: &mut Interp, et: &Value, dir_info: &Value) -> ER<Value> {
    let raf = dir_info.get("RAF");
    let data_pt = dir_info.get("DataPt");
    pl::at(1123);
    let dir_len = pl::length(&pl::sderef(&data_pt)?);
    let sections = Value::new_array(Vec::new());
    let mut dir_info2: Option<Value> = None;
    let mut rsrc_found = false;

    // loop through all sections
    let mut pos = n(0);
    while pl::le(&pl::add(&pos, &n(40)), &dir_len) {
        'body: {
            let name = substr(it, &data_pt.deref(), &pos, Some(&n(8)))?;
            let va = call(it, "Get32u", vec![data_pt.clone(), pl::add(&pos, &n(12))])?;
            let size = call(it, "Get32u", vec![data_pt.clone(), pl::add(&pos, &n(16))])?;
            let offset = call(it, "Get32u", vec![data_pt.clone(), pl::add(&pos, &n(20))])?;
            // remember the section offsets for the VirtualAddress lookup later
            sections.push(hash(&[("Base", offset.clone()), ("Size", size.clone()), ("VirtualAddress", va)]));
            // save details of the first resource section (or .text if .rsrc not found, ref forum11465)
            let take = if pl::str_eq(&name, ".rsrc\0\0\0") && !rsrc_found {
                rsrc_found = true;
                true
            } else {
                pl::str_eq(&name, ".text\0\0\0") && dir_info2.is_none()
            };
            if !take {
                break 'body;
            }
            dir_info2 = Some(hash(&[
                ("RAF", raf.clone()),
                ("Base", offset),
                ("DirStart", n(0)), // (relative to Base)
                ("DirLen", size),
                ("Sections", sections.clone()),
            ]));
        }
        pos = pl::add(&pos, &n(40));
    }
    dir_info.set("Sections", sections); // return section information
    // process the first resource section
    if let Some(d) = dir_info2 {
        if !process_pe_resources_impl(it, et, &d)?.truthy() {
            return Ok(n(0));
        }
    }
    Ok(n(1))
}

/// Process Windows PE file data dictionary
/// Inputs: 0) ExifTool object ref, 1) dirInfo ref
/// Returns: true on success or if the PE resources didn't exist, or false on error
///          processing the PE resources
pub fn process_pe_dict(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    ret(process_pe_dict_impl(it, &arg(&a, 0), &arg(&a, 1))?)
}

fn mach_override_impl(it: &mut Interp, et: &Value, obj_type: &Value, fat: &Value) -> ER<Value> {
    let mach_override = runtime::lex_hash(it, MODULE, "machOverride");
    let over = mach_override.get(obj_type.to_bytes());
    if over.truthy() {
        let desc = cat(&[&pl::s("Mach-O "), &pl::s(if fat.truthy() { "fat " } else { "" }), &over.at(0)]);
        return pl::method(it, et, "OverrideFileType", vec![desc, Value::Undef, over.at(1)]);
    }
    Ok(over)
}

/// Override file type if necessary for Mach object files and libraries
/// Inputs: 0) ExifTool ref, 1) ObjectFileType number, 2) flag for fat binary
pub fn mach_override(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    ret(mach_override_impl(it, &arg(&a, 0), &arg(&a, 1), &arg(&a, 2))?)
}

fn extract_mach_tags_impl(it: &mut Interp, et: &Value, data_pt: &Value, do_obj: &Value) -> ER<Value> {
    // get information about mach header based on the magic number (first 4 bytes)
    let mach_type = runtime::lex_hash(it, MODULE, "machType");
    pl::at(1186);
    let key = pl::substr(&pl::sderef(data_pt)?, 0, Some(4));
    let info = mach_type.get(key.to_bytes());
    if info.truthy() {
        // Mach header structure:
        //  0 int32u magic
        //  4 int32u cputype
        //  8 int32u cpusubtype
        // 12 int32u filetype
        // 16 int32u ncmds
        // 20 int32u sizeofcmds
        // 24 int32u flags
        let tag_table_ptr = call(it, "GetTagTable", vec![pl::s("Image::ExifTool::EXE::MachO")])?;
        call(it, "SetByteOrder", vec![info.at(1)])?;
        let cpu_type = call(it, "Get32s", vec![data_pt.clone(), n(4)])?;
        let sub_type = call(it, "Get32s", vec![data_pt.clone(), n(8)])?;
        handle_tag(it, et, &tag_table_ptr, n(0), info.at(0), &[])?;
        handle_tag(it, et, &tag_table_ptr, n(1), info.at(1), &[])?;
        handle_tag(it, et, &tag_table_ptr, n(3), cpu_type.clone(), &[])?;
        handle_tag(it, et, &tag_table_ptr, n(4), cat(&[&cpu_type, &pl::s(" "), &sub_type]), &[])?;
        if do_obj.truthy() {
            let obj_type = call(it, "Get32u", vec![data_pt.clone(), n(12)])?;
            let flags = call(it, "Get32u", vec![data_pt.clone(), n(24)])?;
            handle_tag(it, et, &tag_table_ptr, n(5), obj_type.clone(), &[])?;
            handle_tag(it, et, &tag_table_ptr, n(6), flags, &[])?;
            // override file type if this is an object file or library
            mach_override_impl(it, et, &obj_type, &Value::Undef)?;
        } else {
            // otherwise this was a static library
            pl::method(it, et, "OverrideFileType", vec![pl::s("Mach-O static library"), Value::Undef, pl::s("A")])?;
        }
        return Ok(n(1));
    }
    Ok(n(0))
}

/// Extract tags from Mach header
/// Inputs: 0) ExifTool ref, 1) data ref, 2) flag to extract object type
/// Returns: true if Mach header was found
pub fn extract_mach_tags(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    ret(extract_mach_tags_impl(it, &arg(&a, 0), &arg(&a, 1), &arg(&a, 2))?)
}

/// Extract information from an EXE file
/// Inputs: 0) ExifTool object reference, 1) dirInfo reference
/// Returns: 1 on success, 0 if this wasn't a valid EXE file
pub fn process_exe(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info) = (arg(&a, 0), arg(&a, 1));
    let raf = dir_info.get("RAF");
    let (buff, buf2) = (cell(Value::Undef), cell(Value::Undef));
    let (mut type_, mut mime, mut ext, mut tag_table_ptr) = (Value::Undef, Value::Undef, Value::Undef, Value::Undef);

    pl::at(1229);
    raf_defined(&raf, "Read")?;
    let mut size = read(it, &raf, &buff, n(0x40))?;
    if !size.truthy() {
        return ret(n(0));
    }
    let fs = et.get("OPTIONS").get("FastScan");
    let fast3 = fs.truthy() && pl::num_eq(&fs, &n(3));
    //
    // DOS and Windows EXE
    //
    let mach_m = re(r"^(\xca\xfe\xba\xbe|\xfe\xed\xfa(\xce|\xcf)|(\xce|\xcf)\xfa\xed\xfe)", "").m(&val(&buff));
    if re(r"^MZ", "").is_match(&val(&buff)) && pl::num_eq(&size, &n(0x40)) {
        // DOS/Windows executable
        // validate DOS header
        // (ref http://www.delphidabbler.com/articles?article=8&part=2)
        //   0 int16u     magic    - Magic number ("MZ")
        //   2 int16u     cblp     - Bytes on last page of file
        //   4 int16u     cp       - Pages in file
        //   6 int16u     crlc     - Relocations
        //   8 int16u     cparhdr  - Size of header in paragraphs
        //  10 int16u     minalloc - Minimum extra paragraphs needed
        //  12 int16u     maxalloc - Maximum extra paragraphs needed
        //  14 int16u     ss       - Initial (relative) SS value
        //  16 int16u     sp       - Initial SP value
        //  18 int16u     csum     - Checksum
        //  20 int16u     ip       - Initial IP value
        //  22 int16u     cs       - Initial (relative) CS value
        //  24 int16u     lfarlc   - Address of relocation table
        //  26 int16u     ovno     - Overlay number
        //  28 int16u[4]  res      - Reserved words
        //  36 int16u     oemid    - OEM identifier (for oeminfo)
        //  38 int16u     oeminfo  - OEM info; oemid specific
        //  40 int16u[10] res2     - Reserved words
        //  60 int32u;    lfanew   - File address of new exe header
        call(it, "SetByteOrder", vec![pl::s("II")])?;
        let u = pl::unpack("x2v2x18vx34V", &val(&buff))?;
        let (cblp, cp, _lfarlc, lfanew) = (arg(&u, 0), arg(&u, 1), arg(&u, 2), arg(&u, 3));
        let file_size = pl::add(&pl::mul(&pl::sub(&cp, &n(if cblp.truthy() { 1 } else { 0 })), &n(512)), &cblp);
        //(patch to accommodate observed 64-bit files)
        //return 0 if $fileSize < 0x40 or $fileSize < $lfarlc;
        //return 0 if $fileSize < 0x40; (changed to warning in ExifTool 12.08)
        if pl::lt(&file_size, &n(0x40)) {
            warn(it, &et, "Invalid file size in DOS header")?;
        }
        // read the Windows NE, PE or LE (virtual device driver) header
        //if ($lfarlc == 0x40 and $fileSize > $lfanew + 2 and ...
        let m = if seek(it, &raf, lfanew, 0)? && read(it, &raf, &buff, n(0x40))?.truthy() { re(r"^(NE|PE|LE)", "").m(&val(&buff)) } else { None };
        if let Some(m) = m {
            let one = cap(&m, 1);
            if pl::str_eq(&one, "NE") {
                if pl::ge(&size, &n(0x40)) {
                    // NE header is 64 bytes (ref 2)
                    // check for DLL
                    let app_flags = call(it, "Get16u", vec![rf(&buff), n(0x0c)])?;
                    ext = pl::s(if band(&app_flags, 0x80).truthy() { "DLL" } else { "EXE" });
                    type_ = cat(&[&pl::s("Win16 "), &ext]);
                    // offset 0x02 is 2 bytes with linker version and revision numbers
                    // offset 0x36 is executable type (2 = Windows)
                }
            } else if pl::str_eq(&one, "PE") {
                // PE header comes at byte 4 in buff:
                //   4 int16u Machine
                //   6 int16u NumberOfSections
                //   8 int32u TimeDateStamp
                //  12 int32u PointerToSymbolTable
                //  16 int32u NumberOfSymbols
                //  20 int16u SizeOfOptionalHeader
                //  22 int16u Characteristics
                if pl::ge(&size, &n(24)) {
                    // PE header is 24 bytes (plus optional header)
                    let mach = call(it, "Get16u", vec![rf(&buff), n(4)])?; // MachineType
                    let flags = call(it, "Get16u", vec![rf(&buff), n(22)])?; // ImageFileCharacteristics
                    let main = pl::pkg_hash(it, "Image::ExifTool::EXE::Main");
                    let mc = main.get("0").get("PrintConv").get(mach.to_bytes());
                    let machine = if mc.truthy() { mc } else { pl::s("") };
                    let win_type = pl::s(if re("64", "").is_match(&machine) { "Win64" } else { "Win32" });
                    ext = pl::s(if band(&flags, 0x2000).truthy() { "DLL" } else { "EXE" });
                    pl::method(it, &et, "SetFileType", vec![cat(&[&win_type, &pl::s(" "), &ext]), Value::Undef, ext.clone()])?;
                    if fast3 {
                        return ret(n(1));
                    }
                    // read the rest of the optional header if necessary
                    let opt_size = call(it, "Get16u", vec![rf(&buff), n(20)])?;
                    let more = pl::sub(&pl::add(&opt_size, &n(24)), &size);
                    let mut magic = n(0);
                    if pl::gt(&more, &n(0)) {
                        if pl::num_eq(&read(it, &raf, &buf2, more.clone())?, &more) {
                            set(&buff, pl::concat(&val(&buff), &val(&buf2)));
                            size = pl::add(&size, &more);
                            magic = call(it, "Get16u", vec![rf(&buff), n(24)])?;
                            // verify PE magic number
                            if !(pl::num_eq(&magic, &n(0x107)) || pl::num_eq(&magic, &n(0x10b)) || pl::num_eq(&magic, &n(0x20b))) {
                                warn(it, &et, "Unknown PE magic number")?;
                                return ret(n(1));
                            }
                            // --> 64-bit if $magic is 0x20b ????
                        } else {
                            warn(it, &et, "Error reading optional header")?;
                        }
                    }
                    // process PE COFF file header
                    tag_table_ptr = call(it, "GetTagTable", vec![pl::s("Image::ExifTool::EXE::Main")])?;
                    let tell = pl::method(it, &raf, "Tell", vec![])?;
                    let d = hash(&[
                        ("DataPt", rf(&buff)),
                        ("DataPos", pl::sub(&tell, &size)),
                        ("DataLen", size.clone()),
                        ("DirStart", n(4)),
                        ("DirLen", pl::sub(&size, &n(4))),
                    ]);
                    pl::method(it, &et, "ProcessDirectory", vec![d, tag_table_ptr.clone()])?;
                    let num = call(it, "Get16u", vec![rf(&buff), n(6)])?; // NumberOfSections
                    // image data directory entry for debug info is index 6,
                    // so offset is 4 bytes + 20 byte header + 96 bytes into PE32 optional
                    // header (or 112 bytes for PE32+) + 6 * 8 bytes into data directory
                    let dir_entry = n(if pl::num_eq(&magic, &n(0x20b)) { 184 } else { 168 });
                    let (mut addr, mut len) = (Value::Undef, Value::Undef); // virtual address/size of debug section
                    let buf2 = cell(Value::Undef);
                    if pl::ge(&pl::length(&val(&buff)), &pl::add(&dir_entry, &n(8))) {
                        addr = call(it, "Get32u", vec![rf(&buff), dir_entry.clone()])?;
                        len = call(it, "Get32u", vec![rf(&buff), pl::add(&dir_entry, &n(4))])?;
                    }
                    // process data dictionary
                    // (ref https://www.debuginfo.com/articles/debuginfomatch.html)
                    let want = pl::mul(&n(40), &num);
                    if !pl::num_eq(&read(it, &raf, &buff, want.clone())?, &want) {
                        return ret(n(1));
                    }
                    let d = hash(&[("RAF", raf.clone()), ("DataPt", rf(&buff))]);
                    if !process_pe_dict_impl(it, &et, &d)?.truthy() {
                        warn(it, &et, "Error processing PE resources")?;
                    }
                    // dive into debug section to extract pdb info if available
                    if !(addr.truthy() && pl::lt(&len, &n(2800)) && d.get("Sections").truthy() && (pl::num_eq(&magic, &n(0x10b)) || pl::num_eq(&magic, &n(0x20b)))) {
                        return ret(n(1));
                    }
                    // get file offset for debug section
                    let off = get_file_offset_impl(&addr, &d.get("Sections"));
                    if !(off.truthy() && seek(it, &raf, off, 0)? && pl::num_eq(&read(it, &raf, &buff, len.clone())?, &len)) {
                        return ret(n(1));
                    }
                    let mut pos = n(0);
                    while pl::le(&pl::add(&pos, &n(28)), &len) {
                        'body: {
                            let type_ = call(it, "Get32u", vec![rf(&buff), pl::add(&pos, &n(12))])?;
                            if !(pl::num_eq(&type_, &n(2)) || pl::num_eq(&type_, &n(4))) {
                                break 'body; // (CodeView debug data)
                            }
                            let nn = call(it, "Get32u", vec![rf(&buff), pl::add(&pos, &n(16))])?;
                            let of = call(it, "Get32u", vec![rf(&buff), pl::add(&pos, &n(24))])?;
                            if !(pl::lt(&nn, &Value::Num(1e4)) && seek(it, &raf, of.clone(), 0)? && pl::num_eq(&read(it, &raf, &buf2, nn.clone())?, &nn)) {
                                break 'body;
                            }
                            if pl::num_eq(&type_, &n(2)) {
                                // CodeView debug info
                                let Some(m) = re(r"^(RSDS|NB10)", "").m(&val(&buf2)) else {
                                    break 'body;
                                };
                                tag_table_ptr = call(it, "GetTagTable", vec![cat(&[&pl::s("Image::ExifTool::EXE::Debug"), &cap(&m, 1)])])?;
                                // use timestamp from debug dir
                                let ts = substr(it, &val(&buff), &pl::add(&pos, &n(4)), Some(&n(4)))?;
                                let rest = pl::substr(&val(&buf2), 4, None);
                                set(&buf2, pl::concat(&ts, &rest));
                                let d = hash(&[("DataPt", rf(&buf2)), ("DataPos", of.clone())]);
                                pl::method(it, &et, "ProcessDirectory", vec![d, tag_table_ptr.clone()])?;
                            } else {
                                // misc debug info
                                if !pl::gt(&nn, &n(12)) {
                                    break 'body;
                                }
                                let mut exe = substr(it, &val(&buf2), &n(12), None)?;
                                if call(it, "Get32u", vec![rf(&buf2), n(8)])?.truthy() {
                                    exe = pl::method(it, &et, "Decode", vec![exe, pl::s("UTF16")])?;
                                }
                                re(r"\0.*", "").subst(&mut exe, false, |_| Vec::new()); // truncate at null
                                tag_table_ptr = call(it, "GetTagTable", vec![pl::s("Image::ExifTool::EXE::Misc")])?;
                                handle_tag(it, &et, &tag_table_ptr, n(12), exe, &[("DataPt", rf(&buf2)), ("DataPos", of.clone())])?;
                            }
                        }
                        pos = pl::add(&pos, &n(28));
                    }
                    return ret(n(1));
                }
            } else {
                type_ = pl::s("Virtual Device Driver");
                ext = pl::s("386");
            }
        } else {
            type_ = pl::s("DOS EXE");
            ext = pl::s("exe");
        }
    //
    // Mach-O (Mac OS X)
    //
    } else if mach_m.is_some() && pl::gt(&size, &n(12)) {
        let m = mach_m.unwrap();
        // Mach-O executable
        // (ref http://developer.apple.com/documentation/DeveloperTools/Conceptual/MachORuntime/Reference/reference.html)
        tag_table_ptr = call(it, "GetTagTable", vec![pl::s("Image::ExifTool::EXE::MachO")])?;
        if &cap(&m, 1).to_bytes()[..] == b"\xca\xfe\xba\xbe" {
            call(it, "SetByteOrder", vec![pl::s("MM")])?;
            let ver = call(it, "Get32u", vec![rf(&buff), n(4)])?;
            // Java bytecode .class files have the same magic number, so we need to look deeper
            // (ref https://github.com/file/file/blob/master/magic/Magdir/cafebabe#L6-L15)
            if pl::gt(&ver, &n(30)) {
                // this is Java bytecode
                pl::method(it, &et, "SetFileType", vec![pl::s("Java bytecode"), pl::s("application/java-byte-code"), pl::s("class")])?;
                return ret(n(1));
            }
            pl::method(it, &et, "SetFileType", vec![pl::s("Mach-O fat binary executable"), Value::Undef, pl::s("")])?;
            if fast3 {
                return ret(n(1));
            }
            let count = call(it, "Get32u", vec![rf(&buff), n(4)])?; // get architecture count
            let more = pl::sub(&pl::mul(&count, &n(20)), &pl::sub(&size, &n(8)));
            if pl::gt(&more, &n(0)) {
                if !pl::num_eq(&read(it, &raf, &buf2, more.clone())?, &more) {
                    warn(it, &et, "Error reading fat-arch headers")?;
                    return ret(n(1));
                }
                set(&buff, pl::concat(&val(&buff), &val(&buf2)));
                size = pl::add(&size, &more);
            }
            handle_tag(it, &et, &tag_table_ptr, n(2), count.clone(), &[])?;
            let mut i = n(0);
            while pl::lt(&i, &count) {
                let cpu_type = call(it, "Get32s", vec![rf(&buff), pl::add(&n(8), &pl::mul(&i, &n(20)))])?;
                let sub_type = call(it, "Get32s", vec![rf(&buff), pl::add(&n(12), &pl::mul(&i, &n(20)))])?;
                handle_tag(it, &et, &tag_table_ptr, n(3), cpu_type.clone(), &[])?;
                handle_tag(it, &et, &tag_table_ptr, n(4), cat(&[&cpu_type, &pl::s(" "), &sub_type]), &[])?;
                i = pl::add(&i, &n(1));
            }
            // load first Mach-O header to get the object file type
            let offset = call(it, "Get32u", vec![rf(&buff), n(16)])?;
            if seek(it, &raf, offset, 0)? && pl::num_eq(&read(it, &raf, &buf2, n(16))?, &n(16)) {
                if re(r"^(\xfe\xed\xfa(\xce|\xcf)|(\xce|\xcf)\xfa\xed\xfe)", "").is_match(&val(&buf2)) {
                    let order = if re(r"^\xfe\xed", "").is_match(&val(&buf2)) { "MM" } else { "II" };
                    call(it, "SetByteOrder", vec![pl::s(order)])?;
                    let obj_type = call(it, "Get32u", vec![rf(&buf2), n(12)])?;
                    handle_tag(it, &et, &tag_table_ptr, n(5), obj_type.clone(), &[])?;
                    // override file type if this is a library or object file
                    mach_override_impl(it, &et, &obj_type, &pl::s("fat"))?;
                } else if re(r"^!<arch>\x0a", "").is_match(&val(&buf2)) {
                    // .a libraries use this magic number
                    handle_tag(it, &et, &tag_table_ptr, n(5), n(-1), &[])?;
                    // override file type since this is a library
                    pl::method(it, &et, "OverrideFileType", vec![pl::s("Mach-O fat static library"), Value::Undef, pl::s("A")])?;
                } else {
                    warn(it, &et, "Unrecognized object file type")?;
                }
            } else {
                warn(it, &et, "Error reading file")?;
            }
        } else if pl::ge(&size, &n(16)) {
            pl::method(it, &et, "SetFileType", vec![pl::s("Mach-O executable"), Value::Undef, pl::s("")])?;
            if fast3 {
                return ret(n(1));
            }
            extract_mach_tags_impl(it, &et, &rf(&buff), &n(1))?;
        }
        return ret(n(1));
    //
    // PEF (classic MacOS)
    //
    } else if re(r"^Joy!peff", "").is_match(&val(&buff)) && pl::gt(&size, &n(12)) {
        // ref http://developer.apple.com/documentation/mac/pdf/MacOS_RT_Architectures.pdf
        pl::method(it, &et, "SetFileType", vec![pl::s("Classic MacOS executable"), Value::Undef, pl::s("")])?;
        if fast3 {
            return ret(n(1));
        }
        call(it, "SetByteOrder", vec![pl::s("MM")])?;
        tag_table_ptr = call(it, "GetTagTable", vec![pl::s("Image::ExifTool::EXE::PEF")])?;
        let d = hash(&[("DataPt", rf(&buff)), ("DataPos", n(0)), ("DataLen", size.clone()), ("DirStart", n(0)), ("DirLen", size.clone())]);
        pl::method(it, &et, "ProcessDirectory", vec![d, tag_table_ptr])?;
        return ret(n(1));
    //
    // ELF (Unix)
    //
    } else if re(r"^\x7fELF", "").is_match(&val(&buff)) && pl::ge(&size, &n(16)) {
        pl::method(it, &et, "SetFileType", vec![pl::s("ELF executable"), Value::Undef, pl::s("")])?;
        if fast3 {
            return ret(n(1));
        }
        let b5 = call(it, "Get8u", vec![rf(&buff), n(5)])?;
        call(it, "SetByteOrder", vec![pl::s(if pl::num_eq(&b5, &n(1)) { "II" } else { "MM" })])?;
        tag_table_ptr = call(it, "GetTagTable", vec![pl::s("Image::ExifTool::EXE::ELF")])?;
        let d = hash(&[("DataPt", rf(&buff)), ("DataPos", n(0)), ("DataLen", size.clone()), ("DirLen", size.clone())]);
        pl::method(it, &et, "ProcessDirectory", vec![d, tag_table_ptr])?;
        // override file type if this is a library or object file
        let oft = et.get("VALUE").get("ObjectFileType");
        let key = if oft.truthy() { oft } else { n(0) };
        let over = match &*key.to_bytes() {
            b"1" => Some(("ELF object file", "O")),
            b"3" => Some(("ELF shared library", "SO")),
            _ => None,
        };
        if let Some((d, e)) = over {
            pl::method(it, &et, "OverrideFileType", vec![pl::s(d), Value::Undef, pl::s(e)])?;
        }
        return ret(n(1));
    //
    // .a libraries
    //
    } else if re(r"^!<arch>\x0a", "").is_match(&val(&buff)) {
        pl::method(it, &et, "SetFileType", vec![pl::s("Static library"), Value::Undef, pl::s("A")])?;
        if fast3 {
            return ret(n(1));
        }
        let mut pos = n(8); // current file position
        let mut max = 10; // maximum number of archive files to check
        // read into list of ar structures (each 60 bytes long):
        loop {
            let go = max > 0;
            max -= 1;
            if !go {
                break;
            }
            // seek to start of the ar structure and read it
            if !(seek(it, &raf, pos.clone(), 0)? && pl::num_eq(&read(it, &raf, &buff, n(60))?, &n(60))) {
                break;
            }
            if !pl::str_eq(&pl::substr(&val(&buff), 58, Some(2)), "`\n") {
                warn(it, &et, "Invalid archive header")?;
                break;
            }
            if !tag_table_ptr.truthy() {
                // extract some information from first file in archive
                tag_table_ptr = call(it, "GetTagTable", vec![pl::s("Image::ExifTool::EXE::AR")])?;
                let d = hash(&[("DataPt", rf(&buff)), ("DataPos", pos.clone())]);
                pl::method(it, &et, "ProcessDirectory", vec![d, tag_table_ptr.clone()])?;
            }
            let name = cell(pl::substr(&val(&buff), 0, Some(16)));
            if let Some(m) = re(r"^#1/(\d+) *$", "").m(&val(&name)) {
                // check for extended archive (BSD variant)
                let len = cap(&m, 1);
                if pl::gt(&len, &n(256)) {
                    warn(it, &et, "Invalid extended archive name length")?;
                    break;
                }
                // (we read the name here just to move the file pointer)
                if !pl::num_eq(&read(it, &raf, &name, len.clone())?, &len) {
                    warn(it, &et, "Error reading archive name")?;
                    break;
                }
            }
            let mut ar_size = pl::substr(&val(&buff), 48, Some(10));
            // make sure archive size is a number
            if re(r"^(\d+).*", "s").subst(&mut ar_size, false, |c| cap(c, 1).to_bytes().to_vec()) == 0 {
                break;
            }
            if !pl::num_eq(&read(it, &raf, &buff, n(28))?, &n(28)) {
                break; // read (possible) Mach header
            }
            if extract_mach_tags_impl(it, &et, &rf(&buff), &Value::Undef)?.truthy() {
                break; // try to extract tags
            }
            pos = pl::add(&pos, &pl::add(&n(60), &ar_size)); // step to next entry
            if band(&pos, 0x01).truthy() {
                pos = pl::add(&pos, &n(1)); // padded to an even byte
            }
        }
        return ret(n(1));
    //
    // various scripts (perl, sh, etc...)
    //
    } else if let Some(m) = re(r"^#!\s*/\S*bin/(\w+)", "").m(&val(&buff)) {
        let mut prog = cap(&m, 1);
        if pl::str_eq(&prog, "env") {
            if let Some(m2) = re(r"\b(perl|python|ruby|php)\b", "").m(&val(&buff)) {
                prog = cap(&m2, 1);
            }
        }
        type_ = cat(&[&prog, &pl::s(" script")]);
        mime = cat(&[&pl::s("text/x-"), &prog]);
        ext = match &*prog.to_bytes() {
            b"perl" => pl::s("pl"),
            b"python" => pl::s("py"),
            b"ruby" => pl::s("rb"),
            b"php" => pl::s("php"),
            _ => Value::Undef,
        };
        // use '.sh' for extension of all shell scripts
        if ext.is_undef() {
            ext = pl::s(if re(r"sh$", "").is_match(&prog) { "sh" } else { "" });
        }
    }
    if !type_.truthy() {
        return ret(n(0));
    }
    pl::method(it, &et, "SetFileType", vec![type_, mime, ext])?;
    ret(n(1))
}

/// Register the subs ported so far; the others still run from the Perl.
pub fn register(it: &mut Interp) {
    it.define_native("Image::ExifTool::EXE::ProcessCHM", process_chm);
    it.define_native("Image::ExifTool::EXE::ReadUnicodeStr", read_unicode_str);
    it.define_native("Image::ExifTool::EXE::ProcessPEVersion", process_pe_version);
    it.define_native("Image::ExifTool::EXE::GetFileOffset", get_file_offset);
    it.define_native("Image::ExifTool::EXE::ProcessPEResources", process_pe_resources);
    it.define_native("Image::ExifTool::EXE::ProcessPEDict", process_pe_dict);
    it.define_native("Image::ExifTool::EXE::MachOverride", mach_override);
    it.define_native("Image::ExifTool::EXE::ExtractMachTags", extract_mach_tags);
    it.define_native("Image::ExifTool::EXE::ProcessEXE", process_exe);
}
