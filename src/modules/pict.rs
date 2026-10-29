//! Image::ExifTool::PICT (Image/ExifTool/PICT.pm), ported sub by sub (see docs/port/PORTING.md).
//!
//! Read PICT meta information. The file's `my` variables ($vers,
//! $extended, $verbose, $out, $indent, @reserved, %structs) stay Perl
//! data, read with `lex_scalar` / `lex_array` / `lex_hash`, so they are
//! shared with the module's table strings as in perl.

use crate::interp::eval::{Cell, ER};
use crate::interp::{Interp, Want};
use crate::modules::random_access::raf_read;
use crate::perl::ops;
use crate::perl::Value;
use crate::pl::{self, arg, cap, n, re, ret};
use crate::runtime;

/// `$raf->Read(...)`: die before the call, as perl does on an undefined or
/// unblessed invocant (the read helper runs the call as interpreted code,
/// which would name its own location in the message).
fn check_invocant(it: &mut Interp, raf: &Value, name: &str) -> ER<()> {
    match raf {
        Value::Str(_) | Value::UStr(_) => Ok(()),
        Value::HashRef(h) if h.borrow().blessed.is_some() => Ok(()),
        _ => pl::method(it, raf, name, vec![]).map(|_| ()),
    }
}

const MODULE: &str = "Image::ExifTool::PICT";

fn s(x: &str) -> Value {
    pl::s(x)
}

/// String interpolation: the parts concatenated.
fn cat(parts: &[Value]) -> Value {
    let mut v = Vec::new();
    for p in parts {
        v.extend_from_slice(&p.to_bytes());
    }
    Value::bytes(v)
}

fn call(it: &mut Interp, name: &str, args: Vec<Value>) -> ER<Value> {
    pl::call(it, &format!("Image::ExifTool::{name}"), args)
}

/// A file lexical of PICT.pm (`my ($vers, $extended)`, `my ($verbose, $out, $indent)`).
fn lex(it: &mut Interp, name: &str) -> Cell {
    runtime::lex_scalar(it, MODULE, name)
}

fn get(c: &Cell) -> Value {
    c.borrow().clone()
}

fn set(c: &Cell, v: Value) {
    *c.borrow_mut() = v;
}

/// `$raf->Read($buff, $len) == $len`
fn read_eq(it: &mut Interp, raf: &Value, buff: &mut Value, len: &Value) -> ER<bool> {
    let r = raf_read(it, raf, buff, len.clone())?;
    Ok(pl::num_eq(&r, len))
}

//------------------------------------------------------------------------------
// Get PixData data
// Inputs: 0) reference to PixMap, 1) RAF reference
// Returns: reference to PixData or undef on error
pub fn get_pix_data(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (pix_map, raf) = (arg(&a, 0), arg(&a, 1));
    let pack_type = pix_map.get("packType");
    let row_bytes = ops::bit_and(&pix_map.get("rowBytes"), &n(0x3fff)); // remove flags bits
    let height = pl::sub(&pix_map.path(&["bounds", "botRight", "v"]), &pix_map.path(&["bounds", "topLeft", "v"]));
    let mut data = Value::Undef;
    let mut buff = Value::Undef;
    let mut size;

    if pl::num_eq(&pack_type, &n(1)) || pl::lt(&row_bytes, &n(8)) {
        // unpacked data
        size = pl::mul(&row_bytes, &height);
        if !read_eq(it, &raf, &mut data, &size)? {
            return pl::ret_undef();
        }
    } else if pl::num_eq(&pack_type, &n(2)) {
        // pad byte dropped
        size = pl::int(&pl::add(&pl::div(&pl::mul(&pl::mul(&row_bytes, &height), &n(3)), &n(4))?, &Value::Num(0.5)));
        if !read_eq(it, &raf, &mut data, &size)? {
            return pl::ret_undef();
        }
    } else {
        data = s("");
        let mut i = n(0);
        while pl::lt(&i, &height) {
            if pl::gt(&row_bytes, &n(250)) {
                if !read_eq(it, &raf, &mut buff, &n(2))? {
                    return pl::ret_undef();
                }
                size = pl::unpack1("n", &buff)?;
            } else {
                if !read_eq(it, &raf, &mut buff, &n(1))? {
                    return pl::ret_undef();
                }
                size = pl::unpack1("C", &buff)?;
            }
            data = pl::concat(&data, &buff);
            if !read_eq(it, &raf, &mut buff, &size)? {
                return pl::ret_undef();
            }
            data = pl::concat(&data, &buff);
            i = pl::add(&i, &n(1));
        }
    }
    ret(pl::sref(data))
}

/// HexDump($valPt, undef, Out => $out [, MaxLen => 96])
fn hex_dump(it: &mut Interp, val_pt: Value, out: &Value, verbose: &Value) -> ER<()> {
    let mut args = vec![val_pt, Value::Undef, s("Out"), out.clone()];
    if pl::lt(verbose, &n(4)) {
        args.push(s("MaxLen"));
        args.push(n(96));
    }
    call(it, "HexDump", args)?;
    Ok(())
}

//------------------------------------------------------------------------------
// Read value from PICT file
// Inputs: 0) RAF reference, 1) tag, 2) format, 3) optional count
// Returns: value, reference to structure hash, or undef on error
pub fn read_pict_value(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (raf, tag, mut format, mut count) = (arg(&a, 0), arg(&a, 1), arg(&a, 2), arg(&a, 3));
    if !format.truthy() {
        return pl::ret_undef();
    }
    if count.is_undef() {
        if let Some(m) = re(r"(.+)\[(.+)\]", "s").m(&format) {
            format = cap(&m, 1);
            count = cap(&m, 2);
        } else {
            count = n(1); // count undefined: assume 1
        }
    }
    let (verbose_c, out_c, indent_c) = (lex(it, "verbose"), lex(it, "out"), lex(it, "indent"));
    let (verbose, out) = (get(&verbose_c), get(&out_c));
    let cnt_str = if pl::num_eq(&count, &n(1)) { s("") } else { cat(&[s("["), count.clone(), s("]")]) };
    // no size if count is 0
    let mut size = if count.truthy() { call(it, "FormatSize", vec![format.clone()])? } else { n(0) };
    if !size.is_undef() || pl::str_eq(&format, "null") {
        let val;
        if size.truthy() {
            let mut buff = Value::Undef;
            size = pl::mul(&size, &count);
            if !read_eq(it, &raf, &mut buff, &size)? {
                return pl::ret_undef();
            }
            val = call(it, "ReadValue", vec![pl::sref(buff), n(0), format.clone(), count.clone(), size.clone()])?;
        } else {
            val = s("");
        }
        if verbose.truthy() {
            pl::print_to(it, Some(&out), &[cat(&[get(&indent_c), tag.clone(), s(" ("), format.clone(), cnt_str.clone(), s(")")])])?;
            if size.truthy() {
                if val.is_undef() {
                    pl::print_to(it, Some(&out), &[s(" = <undef>\n")])?;
                } else if pl::str_eq(&format, "binary") {
                    pl::print_to(it, Some(&out), &[s(" = <binary data>\n")])?;
                    if pl::gt(&verbose, &n(2)) {
                        hex_dump(it, pl::sref(val.clone()), &out, &verbose)?;
                    }
                } else {
                    pl::print_to(it, Some(&out), &[cat(&[s(" = "), val.clone(), s("\n")])])?;
                }
            } else {
                pl::print_to(it, Some(&out), &[s("\n")])?;
            }
        }
        if pl::str_eq(&format, "binary") && !val.is_undef() {
            return ret(pl::sref(val));
        }
        return ret(val);
    }
    if verbose.truthy() {
        pl::print_to(it, Some(&out), &[cat(&[get(&indent_c), tag.clone(), s(" ("), format.clone(), cnt_str, s("):\n")])])?;
    }
    let structs = runtime::lex_hash(it, MODULE, "structs");
    let struct_ = structs.get(format.to_bytes());
    if !struct_.truthy() {
        return pl::ret_undef();
    }
    let mut vals: Vec<Value> = Vec::new();
    let mut c = n(0);
    while pl::lt(&c, &count) {
        let val_h = Value::new_hash(); // (my %val)
        let mut i: i64 = 0;
        loop {
            let tag = struct_.at(i);
            if !tag.truthy() {
                break;
            }
            let mut fmt = struct_.at(i + 1);
            let val;
            set(&indent_c, pl::concat(&get(&indent_c), &s("  ")));
            if fmt.is_ref() {
                // eval $$fmt, with the variables of this scope
                let scope = Some(runtime::file_scope(it, MODULE));
                let vars = [("raf", raf.clone()), ("tag", tag.clone()), ("fmt", fmt.clone()), ("%val", val_h.clone()), ("count", count.clone()), ("format", format.clone())];
                val = pl::eval_site_in(it, &fmt.deref(), MODULE, scope, &vars, Want::Scalar).0;
                let err = it.get_scalar("main::@");
                if err.truthy() {
                    pl::warn(it, &err.to_text())?;
                }
                let verbose = get(&verbose_c);
                if verbose.truthy() && !val.is_undef() {
                    let fmt_s = cat(&[get(&indent_c), tag.clone(), s(" (binary[%d]) = <binary data>\n")]);
                    pl::at(1053);
                    let line = pl::sprintf_v(&fmt_s, &[pl::length(&pl::sderef(&val)?)]);
                    pl::print_to(it, Some(&get(&out_c)), &[line])?;
                    if pl::gt(&verbose, &n(2)) {
                        hex_dump(it, val.clone(), &get(&out_c), &verbose)?;
                    }
                }
            } else if let Some(m) = re(r"(.+)\[(.+)\]", "s").m(&fmt) {
                fmt = cap(&m, 1);
                let scope = Some(runtime::file_scope(it, MODULE));
                let vars = [("raf", raf.clone()), ("tag", tag.clone()), ("fmt", fmt.clone()), ("%val", val_h.clone()), ("count", count.clone()), ("format", format.clone())];
                let cnt = pl::eval_site_in(it, &cap(&m, 2), MODULE, scope, &vars, Want::Scalar).0;
                let err = it.get_scalar("main::@");
                if err.truthy() {
                    pl::warn(it, &err.to_text())?;
                }
                val = read_pict_value(it, vec![raf.clone(), tag.clone(), fmt.clone(), cnt], Want::Scalar)?.pop().unwrap_or_default();
            } else {
                val = read_pict_value(it, vec![raf.clone(), tag.clone(), fmt.clone()], Want::Scalar)?.pop().unwrap_or_default();
            }
            set(&indent_c, pl::substr(&get(&indent_c), 2, None));
            if val.is_undef() {
                return pl::ret_undef();
            }
            val_h.set(tag.to_bytes(), val);
            i += 2;
        }
        if pl::num_eq(&count, &n(1)) {
            return ret(val_h);
        }
        vals.push(val_h);
        c = pl::add(&c, &n(1));
    }
    ret(Value::new_array(vals))
}

//------------------------------------------------------------------------------
// Extract meta information from a PICT image
// Inputs: 0) ExifTool object reference, 1) dirInfo reference
// Returns: 1 on success, 0 if this wasn't a valid PICT image
pub fn process_pict(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info) = (arg(&a, 0), arg(&a, 1));
    let raf = dir_info.get("RAF");
    let (vers_c, extended_c, verbose_c, out_c, indent_c) = (lex(it, "vers"), lex(it, "extended"), lex(it, "verbose"), lex(it, "out"), lex(it, "indent"));
    let verbose = pl::method(it, &et, "Options", vec![s("Verbose")])?;
    set(&verbose_c, verbose.clone());
    let out = pl::method(it, &et, "Options", vec![s("TextOut")])?;
    set(&out_c, out.clone());
    set(&indent_c, s(""));
    let mut buff = Value::Undef;
    let mut tried = false;
    let mut hdr: Vec<Value>;
    let mut op;
    let (mut h_res, mut v_res) = (Value::Undef, Value::Undef);

    // recognize both PICT files and PICT resources (PICT files have a
    // 512-byte header that we ignore, but PICT resources do not)
    loop {
        pl::at(1094);
        check_invocant(it, &raf, "Read")?;
        if !read_eq(it, &raf, &mut buff, &n(12))? {
            return ret(n(0));
        }
        hdr = pl::unpack("x2n5", &buff)?;
        op = hdr.pop().unwrap_or_default();
        // check for PICT version 1 format
        if pl::num_eq(&op, &n(0x1101)) {
            set(&vers_c, n(1));
            set(&extended_c, Value::Undef);
            break;
        }
        // check for PICT version 2 format
        if pl::num_eq(&op, &n(0x0011)) {
            if !read_eq(it, &raf, &mut buff, &n(28))? {
                return ret(n(0));
            }
            if re(r"^\x02\xff\x0c\x00\xff\xff", "").is_match(&buff) {
                set(&vers_c, n(2));
                set(&extended_c, Value::Undef);
                break;
            }
            if re(r"^\x02\xff\x0c\x00\xff\xfe", "").is_match(&buff) {
                set(&vers_c, n(2));
                set(&extended_c, n(1));
                let u = pl::unpack("x8N2", &buff)?;
                (h_res, v_res) = (arg(&u, 0), arg(&u, 1));
                break;
            }
        }
        if tried {
            return ret(n(0));
        }
        tried = true;
        if !pl::method(it, &raf, "Seek", vec![n(512), n(0)])?.truthy() {
            return ret(n(0));
        }
    }
    // make the bounding rect signed
    for x in hdr.iter_mut() {
        if pl::ge(x, &n(0x8000)) {
            *x = pl::sub(x, &n(0x10000));
        }
    }
    let mut w = pl::sub(&arg(&hdr, 3), &arg(&hdr, 1));
    let mut h = pl::sub(&arg(&hdr, 2), &arg(&hdr, 0));
    if !(pl::gt(&w, &n(0)) && pl::gt(&h, &n(0))) {
        return ret(n(0));
    }

    call(it, "SetByteOrder", vec![s("MM")])?;

    if get(&extended_c).truthy() {
        // extended version 2 pictures contain resolution information
        // and image bounds are in 72-dpi equivalent units
        h_res = call(it, "GetFixed32s", vec![pl::sref(buff.clone()), n(8)])?;
        v_res = call(it, "GetFixed32s", vec![pl::sref(buff.clone()), n(12)])?;
        if !(h_res.truthy() && v_res.truthy()) {
            return ret(n(0));
        }
        w = pl::int(&pl::add(&pl::div(&pl::mul(&w, &h_res), &n(72))?, &Value::Num(0.5)));
        h = pl::int(&pl::add(&pl::div(&pl::mul(&h, &v_res), &n(72))?, &Value::Num(0.5)));
    }
    pl::method(it, &et, "SetFileType", vec![])?;
    pl::method(it, &et, "FoundTag", vec![s("ImageWidth"), w])?;
    pl::method(it, &et, "FoundTag", vec![s("ImageHeight"), h])?;
    if h_res.truthy() {
        pl::method(it, &et, "FoundTag", vec![s("XResolution"), h_res.clone()])?;
    }
    if v_res.truthy() {
        pl::method(it, &et, "FoundTag", vec![s("YResolution"), v_res.clone()])?;
    }

    // don't extract image opcodes unless verbose
    if !(verbose.truthy() || pl::method(it, &et, "Options", vec![s("Unknown")])?.truthy()) {
        return ret(n(1));
    }

    let vers = get(&vers_c);
    if verbose.truthy() {
        let fmt = cat(&[s("PICT version "), vers.clone(), s("%s\n")]);
        let line = pl::sprintf_v(&fmt, &[s(if get(&extended_c).truthy() { " extended" } else { "" })]);
        pl::print_to(it, Some(&out), &[line])?;
    }

    let tag_table_ptr = call(it, "GetTagTable", vec![s("Image::ExifTool::PICT::Main")])?;
    let reserved = runtime::lex_array(it, MODULE, "reserved");

    let mut success = false;
    loop {
        let vers = get(&vers_c);
        if pl::num_eq(&vers, &n(1)) {
            if !read_eq(it, &raf, &mut buff, &n(1))? {
                break;
            }
            op = n(buff.to_bytes().first().copied().unwrap_or(0) as i64);
        } else {
            // must start version 2 opcode on an even byte
            let t = pl::method(it, &raf, "Tell", vec![])?;
            if ops::bit_and(&t, &n(0x01)).truthy() {
                raf_read(it, &raf, &mut buff, n(1))?;
            }
            if !read_eq(it, &raf, &mut buff, &n(2))? {
                break;
            }
            op = pl::unpack1("n", &buff)?;
        }
        let mut tag_info = pl::method(it, &et, "GetTagInfo", vec![tag_table_ptr.clone(), op.clone()])?;
        if !tag_info.truthy() {
            // search for reserved tag info
            let mut i = 0;
            while i < reserved.len() as i64 {
                if !pl::ge(&op, &reserved.at(i)) {
                    i += 2;
                    continue;
                }
                if pl::gt(&op, &reserved.at(i + 1)) {
                    break;
                }
                tag_info = pl::method(it, &et, "GetTagInfo", vec![tag_table_ptr.clone(), reserved.at(i)])?;
                break;
            }
            if !tag_info.truthy() {
                break;
            }
        }
        if pl::num_eq(&op, &n(0xff)) {
            if verbose.truthy() {
                pl::print_to(it, Some(&out), &[s("End of picture\n")])?;
            }
            success = true;
            break;
        }
        let mut format = tag_info.get("Format");
        if !format.truthy() {
            pl::method(it, &et, "Warn", vec![cat(&[s("Missing format for "), tag_info.get("Name")])])?;
            break;
        }
        // replace version number for version-dependent formats
        let vb = vers.to_bytes();
        re(r"#$", "").subst(&mut format, false, |_| vb.to_vec());
        let wid = pl::mul(&vers, &n(2));
        if verbose.truthy() {
            let fmt = cat(&[s("Tag 0x%."), wid, s("x, ")]);
            let line = pl::sprintf_v(&fmt, &[op.clone()]);
            pl::print_to(it, Some(&out), &[line])?;
        }
        let val = read_pict_value(it, vec![raf.clone(), tag_info.get("Name"), format], Want::Scalar)?.pop().unwrap_or_default();
        if val.is_undef() {
            pl::method(it, &et, "Warn", vec![cat(&[s("Error reading "), tag_info.get("Name"), s(" information")])])?;
            break;
        }
        if pl::ref_of(&val) == "HASH" {
            // extract JPEG image from CompressedQuickTime imageData
            let descr = val.get("imageDescr");
            if pl::str_eq(&tag_info.get("Name"), "CompressedQuickTime")
                && pl::ref_of(&descr) == "HASH"
                && descr.get("compressor").truthy()
                && pl::str_eq(&descr.get("compressor"), "Photo - JPEG")
                && pl::ref_of(&val.get("imageData")) == "SCALAR"
                && pl::method(it, &et, "ValidateImage", vec![val.get("imageData"), s("PreviewImage")])?.truthy()
            {
                pl::method(it, &et, "FoundTag", vec![s("PreviewImage"), val.get("imageData")])?;
            }
        } else {
            // $et->FoundTag($tagInfo, $val);
        }
    }
    if !success {
        pl::method(it, &et, "Warn", vec![s("End of picture not found")])?;
    }
    ret(n(1))
}

pub fn register(it: &mut Interp) {
    it.define_native("Image::ExifTool::PICT::GetPixData", get_pix_data);
    it.define_native("Image::ExifTool::PICT::ReadPictValue", read_pict_value);
    it.define_native("Image::ExifTool::PICT::ProcessPICT", process_pict);
}
