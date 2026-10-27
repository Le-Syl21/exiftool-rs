//! Image::ExifTool::LNK (Image/ExifTool/LNK.pm), ported sub by sub (see docs/port/PORTING.md).
//!
//! Read meta information from MS Shell Link files

// (the Perl assigns values it does not read again; the port keeps them)
#![allow(unused_assignments)]

use crate::interp::eval::{Cell, ER};
use crate::interp::{Interp, Want};
use crate::modules::random_access::{cell, raf_read_cell, substr};
use crate::perl::regex as pre;
use crate::perl::Value;
use crate::pl::{self, arg, cap, n, re, ret};

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

/// `$raf->Read($buff, $len)` into the `my $buff` cell.
fn read(it: &mut Interp, raf: &Value, buff: &Cell, len: Value) -> ER<Value> {
    raf_read_cell(it, raf, buff, len)
}

/// `$raf->ReadLine($buff)` into the `my $buff` cell.
fn read_line(it: &mut Interp, raf: &Value, buff: &Cell) -> ER<Value> {
    let mut a = [val(buff)];
    let r = pl::method_mut(it, raf, "ReadLine", &mut a, Want::Scalar)?;
    *buff.borrow_mut() = a[0].clone();
    Ok(r.into_iter().next().unwrap_or_default())
}

fn warn(it: &mut Interp, et: &Value, msg: Value) -> ER<()> {
    pl::method(it, et, "Warn", vec![msg])?;
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

/// `$et->HandleTag(TABLE, TAG, VAL, OPTS...)`
fn handle_tag(it: &mut Interp, et: &Value, table: &Value, tag: Value, v: Value, opts: &[(&str, Value)]) -> ER<Value> {
    let mut args = vec![table.clone(), tag, v];
    for (k, x) in opts {
        args.push(pl::s(k));
        args.push(x.clone());
    }
    pl::method(it, et, "HandleTag", args)
}

/// Get DOS date/time
/// Inputs: 0) date/time integer value (date in low word)
/// Returns: EXIF-format date/time string
pub fn dos_time(_it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let v = arg(&a, 0).to_i64() as u64;
    ret(pl::sprintf(
        "%.4d:%.2d:%.2d %.2d:%.2d:%.2d",
        &[
            n((((v >> 9) & 0x7f) + 1980) as i64), // year
            n(((v >> 5) & 0x0f) as i64),          // month
            n((v & 0x1f) as i64),                 // day
            n(((v >> 27) & 0x1f) as i64),         // hour
            n(((v >> 21) & 0x3f) as i64),         // minute
            n(((v >> 15) & 0x3e) as i64),         // second (2 sec resolution)
        ],
    ))
}

fn get_string_impl(data_pt: &Value, pos: &Value, unicode: &Value) -> Value {
    let data = data_pt.deref();
    if pl::ge(pos, &pl::length(&data)) {
        return Value::Undef;
    }
    // pos($$dataPt) = $pos;
    let b = data.to_bytes();
    let start = pos.to_i64().max(0) as usize;
    let pat: &[u8] = if unicode.truthy() { br"\G((?:..)*?)\0\0" } else { br"\G(.*?)\0" };
    let rx = pre::compile(pat, "s").expect("pattern");
    if let Ok(Some(sp)) = rx.match_at(&b, start) {
        if let Some((s, e)) = sp[1] {
            return Value::bytes(&b[s..e]);
        }
    }
    pl::substr(&data, pos.to_i64(), None)
}

/// Extract null-terminated ASCII or Unicode string from buffer
/// Inputs: 0) buffer ref, 1) start position, 2) flag for unicode string
/// Return: string or undef if start position is outside bounds
pub fn get_string(_it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    ret(get_string_impl(&arg(&a, 0), &arg(&a, 1), &arg(&a, 2)))
}

/// Process item ID data (ref 4)
/// Inputs: 0) ExifTool object reference, 1) dirInfo reference, 2) tag table ref
/// Returns: 1 on success
pub fn process_item_id(it: &mut Interp, a: Vec<Value>, want: Want) -> ER<Vec<Value>> {
    let (et, dir_info, tag_table_ptr) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    let verbose = pl::method(it, &et, "Options", vec![pl::s("Verbose")])?;
    let data_pt = dir_info.get("DataPt");
    let data_len = pl::length(&data_pt.deref());
    let mut pos = n(0);
    let opts = [("DataPt", data_pt.clone()), ("DataPos", dir_info.get("DataPos"))];
    pl::method(it, &et, "VerboseDir", vec![pl::s("ItemID"), Value::Undef, data_len.clone()])?;
    loop {
        if pl::gt(&pl::add(&pos, &n(2)), &data_len) {
            warn(it, &et, pl::s("Missing item ID list terminator"))?;
            break;
        }
        let mut size = call(it, "Get16u", vec![data_pt.clone(), pos.clone()])?;
        if pl::num_eq(&size, &n(0)) {
            let more = pl::sub(&pl::sub(&data_len, &pos), &n(2));
            if more.truthy() {
                warn(it, &et, cat(&[&pl::s("Unknown "), &more, &pl::s(" bytes after item ID list")]))?;
            }
            break;
        } else if pl::lt(&size, &n(4)) {
            warn(it, &et, pl::s("Invalid item ID entry size"))?;
            break;
        }
        let tag = call(it, "Get8u", vec![data_pt.clone(), pl::add(&pos, &n(2))])?; // get item ID
        if pl::gt(&pl::add(&pos, &size), &data_len) {
            warn(it, &et, pl::sprintf("Truncated item with ID 0x%.2x", &[tag.clone()]))?;
            size = pl::sub(&data_len, &pos);
        }
        // 0x20-0x2f, 0x30-0x3f and 0x40-0x4f are ID ranges for the same tags
        if !tag_table_ptr.get(tag.to_bytes()).truthy() {
            let lkup = |k: i64| -> Option<i64> {
                match k {
                    0x20 => Some(0x2e),
                    0x21 => Some(0x2f),
                    0x30 => Some(0x31),
                    0x40 => Some(0x40),
                    _ => None,
                }
            };
            let t = tag.to_i64();
            // (keep bit 0 of MyComputer ID)
            let src_tag = match lkup(t & 0x71).filter(|v| *v != 0).or_else(|| lkup(t & 0x70)) {
                Some(v) => n(v),
                None => Value::Undef,
            };
            // make a clone of the existing tag definition
            let src = tag_table_ptr.get(src_tag.to_bytes());
            if pl::ref_of(&src) == "HASH" {
                let clone = src.hash_copy();
                clone.set("TagID", tag.clone());
                tag_table_ptr.set(tag.to_bytes(), clone);
            }
        }
        if !tag_table_ptr.get(tag.to_bytes()).truthy() {
            let ti = Value::new_hash();
            ti.set("Name", pl::sprintf("Item_%.2x", &[tag.clone()]));
            let sd = Value::new_hash();
            sd.set("TagTable", pl::s("Image::ExifTool::LNK::UnknownData"));
            ti.set("SubDirectory", sd);
            call(it, "AddTagToTable", vec![tag_table_ptr.clone(), tag.clone(), ti])?;
        }
        // isolate this itemID data
        let dat = substr(it, &data_pt.deref(), &pos, Some(&size))?;
        // look for 0xbeef extension blocks
        // (don't know what the first 2 bytes are, but I've seen 0x0003, 0x0007, 0x0008
        //  and 0x0009 for 0xbeef0004, and 0x0000 for 0xbeef0014 and 0xbeef0025)
        // List of observed 0xbeef blocks (0xbeef ID - ItemID):
        //  0xbeef0003 - 0xb1
        //  0xbeef0004 - 0x31,0x32
        //  0xbeef0014 - 0x61
        //  0xbeef0025 - 0x2e
        //  0xbeef0026 - 0x1f,0x2e
        let mut beef_start = Value::Undef;
        {
            let rx = pre::compile(br".{5}\0\xef\xbe", "s").expect("pattern");
            let b = dat.to_bytes();
            if let Ok(Some(sp)) = rx.match_at(&b, 0) {
                let end = sp[0].map(|(_, e)| e).unwrap_or(0) as i64;
                let off = n(end - 8);
                let off2 = call(it, "Get16u", vec![pl::sref(dat.clone()), pl::sub(&pl::length(&dat), &n(2))])?;
                if pl::num_eq(&off, &off2) {
                    beef_start = pl::add(&pos, &off);
                } else {
                    warn(it, &et, pl::s("Unexpected 0xbeef extension offset"))?;
                }
            }
        }
        let sz = if beef_start.truthy() { pl::sub(&beef_start, &pos) } else { size.clone() };
        let mut o: Vec<(&str, Value)> = opts.to_vec();
        o.push(("Start", pos.clone()));
        o.push(("Size", sz));
        handle_tag(it, &et, &tag_table_ptr, tag.clone(), Value::Undef, &o)?;
        if pl::gt(&verbose, &n(1)) && beef_start.truthy() {
            let len = pl::add(&pl::sub(&size, &beef_start), &pos);
            let fmt = cat(&[&et.get("INDENT"), &pl::s("- Tag 0x%.4x Extension (%d bytes)\n")]);
            let msg = pl::sprintf_v(&fmt, &[tag.clone(), len.clone()]);
            pl::method(it, &et, "VPrint", vec![n(1), msg])?;
            let mut args = vec![data_pt.clone()];
            for (k, x) in opts.iter() {
                args.push(pl::s(k));
                args.push(x.clone());
            }
            args.extend([pl::s("Start"), beef_start.clone(), pl::s("Len"), len]);
            pl::method(it, &et, "VerboseDump", args)?;
        }
        // process 0xbeef extension blocks
        while beef_start.truthy() {
            let end = pl::add(&pos, &size); // end if itemID entry
            if pl::gt(&pl::add(&beef_start, &n(8)), &end) {
                if !pl::num_eq(&beef_start, &end) {
                    warn(it, &et, pl::s("Garbage data in 0xbeef extension"))?;
                }
                break;
            }
            let mut len = call(it, "Get16u", vec![data_pt.clone(), beef_start.clone()])?;
            let beef_id = call(it, "Get32u", vec![data_pt.clone(), pl::add(&beef_start, &n(4))])?;
            if !pl::num_eq(&band(&beef_id, 0xffff0000), &n(0xbeef0000)) {
                warn(it, &et, pl::s("Invalid 0xbeef extension"))?;
                break;
            }
            if !pl::le(&pl::add(&beef_start, &len), &end) {
                warn(it, &et, pl::s("Truncated 0xbeef extension"))?;
                break;
            }
            if !tag_table_ptr.get(beef_id.to_bytes()).truthy() {
                let ti = Value::new_hash();
                ti.set("Name", pl::sprintf("Unknown_beef%.4x", &[band(&beef_id, 0xffff)]));
                ti.set("Binary", n(1));
                call(it, "AddTagToTable", vec![tag_table_ptr.clone(), beef_id.clone(), ti])?;
            }
            let mut o: Vec<(&str, Value)> = opts.to_vec();
            o.push(("Start", beef_start.clone()));
            o.push(("Size", len.clone()));
            handle_tag(it, &et, &tag_table_ptr, beef_id, Value::Undef, &o)?;
            if band(&len, 0x01).truthy() {
                len = pl::add(&len, &n(1)); // assume they start on 2-byte boundaries (NC)
            }
            beef_start = pl::add(&beef_start, &len); // step to next extension block
        }
        pos = pl::add(&pos, &size);
    }
    pl::ret_nothing(want)
}

/// Process URI 0xbeef0014 extension (ref 4)
/// Inputs: 0) ExifTool ref, 1) dirInfo ref, 2) tag table ref
/// Returns: 1 on success
pub fn process_uri(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info, tag_table_ptr) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    let data_pt = dir_info.get("DataPt");
    let ds = dir_info.get("DirStart");
    let mut pos = if ds.truthy() { ds } else { n(0) };
    let dir_len = dir_info.get("DirLen");
    let end = pl::add(&pos, &dir_len);
    if pl::lt(&dir_len, &n(56)) {
        return ret(n(0));
    }
    pl::method(it, &et, "VerboseDir", vec![pl::s("URI"), Value::Undef, dir_len.clone()])?;
    // offset 8 - GUID df2fce13-25ec-45bb-9d4c-cecd47c2430c
    let num = call(it, "Get32u", vec![data_pt.clone(), pl::add(&pos, &n(52))])?;
    pos = pl::add(&pos, &n(56));
    let mut i = n(0);
    while pl::lt(&i, &num) {
        'body: {
            if pl::gt(&pl::add(&pos, &n(8)), &end) {
                warn(it, &et, pl::s("Truncated URI extension"))?;
                return ret(n(1));
            }
            let tag = call(it, "Get32u", vec![data_pt.clone(), pos.clone()])?;
            let size = call(it, "Get32u", vec![data_pt.clone(), pl::add(&pos, &n(4))])?;
            pos = pl::add(&pos, &n(8));
            if !size.truthy() {
                break 'body;
            }
            if pl::gt(&pl::add(&pos, &size), &end) {
                warn(it, &et, pl::s("Truncated URI string"))?;
                return ret(n(1));
            }
            // (many tags are untested -- some may not be Unicode strings?)
            let s = substr(it, &data_pt.deref(), &pos, Some(&size))?;
            let v = pl::method(it, &et, "Decode", vec![s, pl::s("UTF16")])?;
            let o = [("DataPt", data_pt.clone()), ("DataPos", dir_info.get("DataPos")), ("Start", pos.clone()), ("Size", size.clone())];
            handle_tag(it, &et, &tag_table_ptr, tag, v, &o)?;
            pos = pl::add(&pos, &size);
        }
        i = pl::add(&i, &n(1));
    }
    ret(n(1))
}

/// Process link information data
/// Inputs: 0) ExifTool object reference, 1) dirInfo reference, 2) tag table ref
/// Returns: 1 on success
pub fn process_link_info(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info, tag_table_ptr) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    let data_pt = dir_info.get("DataPt");
    let data_len = pl::length(&data_pt.deref());
    if pl::lt(&data_len, &n(0x24)) {
        return ret(n(0));
    }
    let hdr_len = call(it, "Get32u", vec![data_pt.clone(), n(4)])?;
    let lif = call(it, "Get32u", vec![data_pt.clone(), n(8)])?; // link info flags
    let opts = [
        ("DataPt", data_pt.clone()),
        ("DataPos", dir_info.get("DataPos")),
        ("Size", n(4)), // (typical value size)
    ];
    // %opts followed by more pairs: later keys win, as in the Perl hash
    let with = |extra: &[(&'static str, Value)]| -> Vec<(&'static str, Value)> {
        let mut o: Vec<(&'static str, Value)> = opts.to_vec();
        for (k, v) in extra {
            if let Some(e) = o.iter_mut().find(|(ok, _)| ok == k) {
                e.1 = v.clone();
            } else {
                o.push((k, v.clone()));
            }
        }
        o
    };
    let (mut off, mut unicode, mut pos, mut v, mut size);
    pl::method(it, &et, "VerboseDir", vec![pl::s("LinkInfo"), Value::Undef, data_len.clone()])?;
    if band(&lif, 0x01).truthy() {
        // read Volume ID
        off = call(it, "Get32u", vec![data_pt.clone(), n(0x0c)])?;
        if off.truthy() && pl::le(&pl::add(&off, &n(0x20)), &data_len) {
            // my $len = Get32u($dataPt, $off);
            handle_tag(it, &et, &tag_table_ptr, pl::s("DriveType"), Value::Undef, &with(&[("Start", pl::add(&off, &n(4)))]))?;
            handle_tag(it, &et, &tag_table_ptr, pl::s("DriveSerialNumber"), Value::Undef, &with(&[("Start", pl::add(&off, &n(8)))]))?;
            pos = call(it, "Get32u", vec![data_pt.clone(), pl::add(&off, &n(0x0c))])?;
            unicode = Value::Undef;
            if pl::num_eq(&pos, &n(0x14)) {
                // use VolumeLabelOffsetUnicode instead
                pos = call(it, "Get32u", vec![data_pt.clone(), pl::add(&off, &n(0x10))])?;
                unicode = n(1);
            }
            pos = pl::add(&pos, &off);
            v = get_string_impl(&data_pt, &pos, &unicode);
            if !v.is_undef() {
                size = pl::length(&v);
                if unicode.truthy() {
                    v = pl::method(it, &et, "Decode", vec![v, pl::s("UTF16")])?;
                }
                handle_tag(it, &et, &tag_table_ptr, pl::s("VolumeLabel"), v, &with(&[("Start", pos.clone()), ("Size", size)]))?;
            }
        } else {
            unicode = Value::Undef;
        }
        // read local base path
        if pl::ge(&hdr_len, &n(0x24)) {
            pos = call(it, "Get32u", vec![data_pt.clone(), n(0x1c)])?;
            unicode = n(1);
        } else {
            pos = call(it, "Get32u", vec![data_pt.clone(), n(0x10)])?;
            unicode = Value::Undef;
        }
        v = get_string_impl(&data_pt, &pos, &unicode);
        if !v.is_undef() {
            size = pl::length(&v);
            if unicode.truthy() {
                v = pl::method(it, &et, "Decode", vec![v, pl::s("UTF16")])?;
            }
            handle_tag(it, &et, &tag_table_ptr, pl::s("LocalBasePath"), v, &with(&[("Start", pos.clone()), ("Size", size)]))?;
        }
    }
    if band(&lif, 0x02).truthy() {
        // read common network relative link
        off = call(it, "Get32u", vec![data_pt.clone(), n(0x14)])?;
        if off.truthy() && pl::le(&pl::add(&off, &n(0x14)), &data_len) {
            let siz = call(it, "Get32u", vec![data_pt.clone(), off.clone()])?;
            if pl::gt(&pl::add(&off, &siz), &data_len) {
                return ret(n(0));
            }
            pos = call(it, "Get32u", vec![data_pt.clone(), pl::add(&off, &n(0x08))])?;
            if pl::gt(&pos, &n(0x14)) && pl::ge(&siz, &n(0x18)) {
                pos = call(it, "Get32u", vec![data_pt.clone(), pl::add(&off, &n(0x14))])?;
                unicode = n(1);
            } else {
                unicode = Value::Undef;
            }
            v = get_string_impl(&data_pt, &pl::add(&off, &pos), &unicode);
            if !v.is_undef() {
                size = pl::length(&v);
                if unicode.truthy() {
                    v = pl::method(it, &et, "Decode", vec![v, pl::s("UTF16")])?;
                }
                handle_tag(it, &et, &tag_table_ptr, pl::s("NetName"), v, &with(&[("Start", pos.clone()), ("Size", size)]))?;
            }
            let flg = call(it, "Get32u", vec![data_pt.clone(), pl::add(&off, &n(0x04))])?;
            if band(&flg, 0x01).truthy() {
                pos = call(it, "Get32u", vec![data_pt.clone(), pl::add(&off, &n(0x0c))])?;
                if pl::gt(&pos, &n(0x14)) && pl::ge(&siz, &n(0x1c)) {
                    pos = call(it, "Get32u", vec![data_pt.clone(), pl::add(&off, &n(0x18))])?;
                    unicode = n(1);
                } else {
                    unicode = Value::Undef;
                }
                v = get_string_impl(&data_pt, &pl::add(&off, &pos), &unicode);
                if !v.is_undef() {
                    size = pl::length(&v);
                    if unicode.truthy() {
                        v = pl::method(it, &et, "Decode", vec![v, pl::s("UTF16")])?;
                    }
                    handle_tag(it, &et, &tag_table_ptr, pl::s("DeviceName"), v, &with(&[("Start", pos.clone()), ("Size", size)]))?;
                }
            }
            if band(&flg, 0x02).truthy() {
                v = call(it, "Get32u", vec![data_pt.clone(), pl::add(&off, &n(0x10))])?;
                handle_tag(it, &et, &tag_table_ptr, pl::s("NetProviderType"), v, &with(&[("Start", pl::add(&off, &n(0x10)))]))?;
            }
        }
    }
    off = call(it, "Get32u", vec![data_pt.clone(), n(0x18)])?;
    if off.truthy() && pl::lt(&off, &data_len) {
        v = get_string_impl(&data_pt, &off, &Value::Undef);
        let sz = pl::add(&pl::length(&v), &n(1));
        handle_tag(it, &et, &tag_table_ptr, pl::s("CommonPathSuffix"), v, &with(&[("Start", off.clone()), ("Size", sz)]))?;
    }
    if pl::ge(&hdr_len, &n(0x24)) {
        off = call(it, "Get32u", vec![data_pt.clone(), n(0x20)])?;
        if off.truthy() && pl::lt(&off, &data_len) {
            v = get_string_impl(&data_pt, &off, &n(1));
            let sz = pl::add(&pl::length(&v), &n(1));
            handle_tag(it, &et, &tag_table_ptr, pl::s("CommonPathSuffixUnicode"), v, &with(&[("Start", off.clone()), ("Size", sz)]))?;
        }
    }
    ret(n(1))
}

fn process_ini_impl(it: &mut Interp, et: &Value, dir_info: &Value) -> ER<Value> {
    let raf = dir_info.get("RAF");
    let buff = cell(Value::Undef);
    pl::local_global(it, b'$', "main::/", |it| {
        it.set_scalar("main::/", Value::bytes(b"\x0d\x0a"));
        let tag_table_ptr = call(it, "GetTagTable", vec![pl::s("Image::ExifTool::LNK::INI")])?;
        while read_line(it, &raf, &buff)?.truthy() {
            let b = val(&buff);
            if let Some(m) = re(r"^\[(.*?)\]", "").m(&b) {
                pl::method(it, et, "VPrint", vec![n(0), cat(&[&cap(&m, 1), &pl::s(" section:\n")])])?;
            } else if let Some(m) = re(r"^\s*(\w+)=(.*)\x0d\x0a$", "").m(&b) {
                handle_tag(it, et, &tag_table_ptr, cap(&m, 1), cap(&m, 2), &[("MakeTagInfo", n(1))])?;
            }
        }
        Ok(n(1))
    })
}

/// Extract information from a INI-format file
/// Inputs: 0) ExifTool object reference, 1) dirInfo reference
/// Returns: 1 on success, 0 if this wasn't a valid INI file
pub fn process_ini(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    ret(process_ini_impl(it, &arg(&a, 0), &arg(&a, 1))?)
}

/// Extract information from a MS Shell Link (Windows shortcut) file
/// Inputs: 0) ExifTool object reference, 1) dirInfo reference
/// Returns: 1 on success, 0 if this wasn't a valid LNK file
pub fn process_lnk(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info) = (arg(&a, 0), arg(&a, 1));
    let raf = dir_info.get("RAF");
    let (buff, buf2) = (cell(Value::Undef), cell(Value::Undef));
    let (mut len, mut i, mut bad, mut is_unicode);
    bad = false;
    is_unicode = false;

    // read LNK file header
    if !pl::num_eq(&read(it, &raf, &buff, n(0x4c))?, &n(0x4c)) {
        return ret(n(0));
    }
    if !re(r"^.{4}\x01\x14\x02\0{5}\xc0\0{6}\x46", "s").is_match(&val(&buff)) {
        // check for INI-format LNK file (eg. .URL file)
        if !re(r"^\[[InternetShortcut\][\x0d\x0a]", "").is_match(&val(&buff)) {
            return ret(n(0));
        }
        if !pl::method(it, &raf, "Seek", vec![n(0), n(0)])?.truthy() {
            return ret(n(0));
        }
        pl::method(it, &et, "SetFileType", vec![pl::s("URL"), pl::s("application/x-mswinurl")])?;
        return ret(process_ini_impl(it, &et, &dir_info)?);
    }
    len = pl::unpack1("V", &val(&buff))?;
    if !pl::ge(&len, &n(0x4c)) {
        return ret(n(0));
    }
    if pl::gt(&len, &n(0x4c)) {
        let want = pl::sub(&len, &n(0x4c));
        if !pl::num_eq(&read(it, &raf, &buf2, want.clone())?, &want) {
            return ret(n(0));
        }
        let b = pl::concat(&val(&buff), &val(&buf2));
        *buff.borrow_mut() = b;
    }
    pl::method(it, &et, "SetFileType", vec![])?;
    call(it, "SetByteOrder", vec![pl::s("II")])?;

    let tag_table_ptr = call(it, "GetTagTable", vec![pl::s("Image::ExifTool::LNK::Main")])?;
    let dir_info2 = Value::new_hash();
    dir_info2.set("DataPt", rf(&buff));
    dir_info2.set("DataPos", n(0));
    dir_info2.set("DataLen", pl::length(&val(&buff)));
    dir_info2.set("DirLen", pl::length(&val(&buff)));
    pl::method(it, &et, "ProcessDirectory", vec![dir_info2, tag_table_ptr.clone()])?;

    let flags = call(it, "Get32u", vec![rf(&buff), n(0x14)])?;

    // save Unicode flag for strings
    if band(&flags, 0x80).truthy() {
        is_unicode = true;
    }

    // read link target ID list
    if band(&flags, 0x01).truthy() {
        if !pl::num_eq(&read(it, &raf, &buff, n(2))?, &n(2)) {
            warn(it, &et, pl::s("Missing target ID list"))?;
            return ret(n(1));
        }
        len = pl::unpack1("v", &val(&buff))?;
        if !pl::num_eq(&read(it, &raf, &buff, len.clone())?, &len) {
            warn(it, &et, pl::s("Truncated target ID list"))?;
            len = pl::length(&val(&buff));
        }
        let tell = pl::method(it, &raf, "Tell", vec![])?;
        handle_tag(it, &et, &tag_table_ptr, n(0x10000), Value::Undef, &[("DataPt", rf(&buff)), ("DataPos", pl::sub(&tell, &len)), ("Size", len.clone())])?;
    }

    // read link information
    if band(&flags, 0x02).truthy() {
        if !pl::num_eq(&read(it, &raf, &buff, n(4))?, &n(4)) {
            warn(it, &et, pl::s("Missing link information"))?;
            return ret(n(1));
        }
        len = pl::unpack1("V", &val(&buff))?;
        if pl::lt(&len, &n(4)) {
            warn(it, &et, pl::s("Invalid link information"))?;
            return ret(n(1));
        }
        let want = pl::sub(&len, &n(4));
        if !pl::num_eq(&read(it, &raf, &buf2, want.clone())?, &want) {
            warn(it, &et, pl::s("Truncated link information"))?;
            len = pl::length(&val(&buf2));
        }
        let b = pl::concat(&val(&buff), &val(&buf2));
        *buff.borrow_mut() = b;
        let tell = pl::method(it, &raf, "Tell", vec![])?;
        handle_tag(it, &et, &tag_table_ptr, n(0x20000), Value::Undef, &[("DataPt", rf(&buff)), ("DataPos", pl::sub(&tell, &len)), ("Size", len.clone())])?;
    }

    // read string data
    let strings = ["Description", "RelativePath", "WorkingDirectory", "CommandLineArguments", "IconFileName"];
    i = 0usize;
    while i < strings.len() {
        'body: {
            let mut v = Value::Undef;
            let mut limit = false;
            let mask = 0x04i64 << i;
            if !band(&flags, mask).truthy() {
                break 'body;
            }
            if !pl::num_eq(&read(it, &raf, &buff, n(2))?, &n(2)) {
                warn(it, &et, pl::s(&format!("Invalid {} string", strings[i])))?;
                return ret(n(1));
            }
            let pos = pl::method(it, &raf, "Tell", vec![])?;
            len = pl::unpack1("v", &val(&buff))?;
            if !len.truthy() {
                break 'body;
            }
            // Windows doesn't follow their own specification and limits the length
            // for most of these strings (ref 3)
            if i != 3 && pl::ge(&len, &n(260)) {
                limit = true;
                if pl::gt(&len, &n(260)) {
                    len = n(260);
                    warn(it, &et, pl::s("LNK string data overrun! Possible security issue"))?;
                }
            }
            // characters are 2 bytes if Unicode flag is set
            if is_unicode {
                len = pl::mul(&len, &n(2));
            }
            if !read(it, &raf, &buff, len.clone())?.truthy() {
                warn(it, &et, pl::s(&format!("Truncated {} string", strings[i])))?;
                return ret(n(1));
            }
            // remove last character if string is at length limit (Windows treats this as a null)
            if limit {
                len = pl::sub(&len, &n(if is_unicode { 2 } else { 1 }));
                let b = substr(it, &val(&buff), &n(0), Some(&len))?;
                *buff.borrow_mut() = b;
            }
            if is_unicode {
                v = pl::method(it, &et, "Decode", vec![val(&buff), pl::s("UTF16")])?;
            }
            handle_tag(it, &et, &tag_table_ptr, n(0x30000 | mask), v, &[("DataPt", rf(&buff)), ("DataPos", pos), ("Size", len.clone())])?;
        }
        i += 1;
    }

    // read extra data
    while pl::num_eq(&read(it, &raf, &buff, n(4))?, &n(4)) {
        len = pl::unpack1("V", &val(&buff))?;
        if pl::lt(&len, &n(4)) {
            break;
        }
        len = pl::sub(&len, &n(4));
        if !pl::num_eq(&read(it, &raf, &buf2, len.clone())?, &len) {
            warn(it, &et, pl::s("Truncated extra data"))?;
            break;
        }
        if !pl::gt(&len, &n(4)) {
            continue;
        }
        let b = pl::concat(&val(&buff), &val(&buf2));
        *buff.borrow_mut() = b;
        let tag = call(it, "Get32u", vec![rf(&buff), n(4)])?;
        let mut tag_info = tag_table_ptr.get(tag.to_bytes());
        if !(pl::ref_of(&tag_info) == "HASH" && tag_info.get("SubDirectory").truthy()) {
            tag_info = tag_table_ptr.get(n(0xa0000000).to_bytes());
        }
        let tell = pl::method(it, &raf, "Tell", vec![])?;
        handle_tag(it, &et, &tag_table_ptr, tag, Value::Undef, &[("DataPt", rf(&buff)), ("DataPos", pl::sub(&pl::sub(&tell, &len), &n(4))), ("TagInfo", tag_info)])?;
    }
    // check for an overlay
    let pos = pl::method(it, &raf, "Tell", vec![])?;
    len = n(0);
    loop {
        let r = read(it, &raf, &buff, n(65536))?;
        if !r.truthy() {
            break;
        }
        len = pl::add(&len, &r);
        if !bad && re(r"[^\0]", "").is_match(&val(&buff)) {
            bad = true;
        }
    }
    if bad {
        warn(it, &et, pl::sprintf("Unknown %d-byte overlay at offset 0x%x", &[len, pos]))?;
    } else if len.truthy() {
        warn(it, &et, cat(&[&len, &pl::s(" bytes of null padding at end of file")]))?;
    }
    ret(n(1))
}

/// Register the subs ported so far; the others still run from the Perl.
pub fn register(it: &mut Interp) {
    it.define_native("Image::ExifTool::LNK::DOSTime", dos_time);
    it.define_native("Image::ExifTool::LNK::GetString", get_string);
    it.define_native("Image::ExifTool::LNK::ProcessItemID", process_item_id);
    it.define_native("Image::ExifTool::LNK::ProcessURI", process_uri);
    it.define_native("Image::ExifTool::LNK::ProcessLinkInfo", process_link_info);
    it.define_native("Image::ExifTool::LNK::ProcessINI", process_ini);
    it.define_native("Image::ExifTool::LNK::ProcessLNK", process_lnk);
}
