//! Image::ExifTool::MIE (Image/ExifTool/MIE.pm), ported sub by sub (see docs/port/PORTING.md).
//!
//! Read/write Meta Information Encapsulation (MIE) files

use std::cmp::Ordering;
use std::collections::HashSet;

use crate::interp::eval::{Cell, ER};
use crate::interp::{Interp, Want};
use crate::modules::random_access::{cell, perl_eval1};
use crate::modules::write_exif::{byte_order, chr, etf, g32u, get, inc, keys, len, put, read, read_value, s16u, s32u, seek, set_byte_order, tell, write, write_value};
use crate::perl::{ops, Value};
use crate::pl::{self, arg, cap, re, ret};

const MODULE: &str = "Image::ExifTool::MIE";

fn n(i: i64) -> Value {
    Value::Int(i)
}

fn s(x: &str) -> Value {
    Value::str(x)
}

/// `"$a$b..."`
fn cat(parts: &[&Value]) -> Value {
    let mut v = Vec::new();
    for p in parts {
        v.extend_from_slice(&p.to_bytes());
    }
    Value::bytes(v)
}

/// `$a eq $b`
fn seq(a: &Value, b: &Value) -> bool {
    a.to_bytes() == b.to_bytes()
}

/// `$a lt $b` / `$a gt $b`
fn slt(a: &Value, b: &Value) -> bool {
    ops::str_cmp(a, b) == Ordering::Less
}

fn sgt(a: &Value, b: &Value) -> bool {
    ops::str_cmp(a, b) == Ordering::Greater
}

/// `$a & $b`
fn band(a: &Value, b: i64) -> Value {
    ops::bit_and(a, &n(b))
}

/// `$raf->Read($buff, LEN)` into a `my` variable: the number of bytes read.
fn raf_read(it: &mut Interp, raf: &Value, buff: &mut Value, l: Value) -> ER<Value> {
    let c = cell(buff.clone());
    let r = read(it, raf, &c, l)?;
    *buff = get(&c);
    Ok(r)
}

/// `$raf->Read($buff, LEN) == LEN`
fn raf_read_eq(it: &mut Interp, raf: &Value, buff: &mut Value, l: &Value) -> ER<bool> {
    let r = raf_read(it, raf, buff, l.clone())?;
    Ok(pl::num_eq(&r, l))
}

/// `print $out LIST`
fn print_out(it: &mut Interp, out: &Value, items: &[Value]) -> ER<()> {
    pl::print_to(it, Some(out), items)?;
    Ok(())
}

/// `tell $fh` for a file handle.
fn tell_fh(it: &mut Interp, fh: &Value) -> ER<Value> {
    let c = cell(fh.clone());
    perl_eval1(it, "tell $fh", MODULE, &[("fh", &c)])
}

/// `ref $outfile eq 'SCALAR' ? length($$outfile) || 0 : tell $outfile`
fn out_pos(it: &mut Interp, outfile: &Value) -> ER<Value> {
    if pl::ref_of(outfile) == "SCALAR" {
        let l = len(&outfile.deref());
        Ok(if l.truthy() { l } else { n(0) })
    } else {
        tell_fh(it, outfile)
    }
}

/// `$inflate->inflate($buff)` in list context: ($out, $status); the input
/// is passed as an alias, as perl does.
fn inflate_call(it: &mut Interp, inflate: &Value, buff: &mut Value) -> ER<(Value, Value)> {
    let mut args = [buff.clone()];
    let r = pl::method_mut(it, inflate, "inflate", &mut args, Want::List)?;
    *buff = args[0].clone();
    Ok((arg(&r, 0), arg(&r, 1)))
}

/// `Compress::Zlib::Z_STREAM_END()`
fn z_stream_end(it: &mut Interp) -> ER<Value> {
    pl::call(it, "Compress::Zlib::Z_STREAM_END", vec![])
}

/// `\&ProcessMIE`
fn process_mie_ref(it: &mut Interp) -> Value {
    Value::Code(it.sub_ref("Image::ExifTool::MIE::ProcessMIE"))
}

/// `$et->Warn(MSG)`
fn warn(it: &mut Interp, et: &Value, msg: Value) -> ER<()> {
    pl::method(it, et, "Warn", vec![msg])?;
    Ok(())
}

/// `$et->Options(NAME)`
fn opt(it: &mut Interp, et: &Value, name: &str) -> ER<Value> {
    pl::method(it, et, "Options", vec![s(name)])
}

/// The file lexicals (`my %mieFormat` ...).
fn mie_format(it: &mut Interp) -> Value {
    crate::runtime::lex_hash(it, MODULE, "mieFormat")
}

fn mie_map(it: &mut Interp) -> Value {
    crate::runtime::lex_hash(it, MODULE, "mieMap")
}

fn mie_code(it: &mut Interp) -> Value {
    crate::runtime::lex_hash(it, MODULE, "mieCode")
}

fn has_zlib_var(it: &mut Interp) -> Cell {
    crate::runtime::lex_scalar(it, MODULE, "hasZlib")
}

fn done_mie_map_var(it: &mut Interp) -> Cell {
    crate::runtime::lex_scalar(it, MODULE, "doneMieMap")
}

/// `$mieFormat{$format & 0xfb} || 'undef'`
fn format_str_of(it: &mut Interp, format: &Value) -> Value {
    let f = mie_format(it).get(band(format, 0xfb).to_bytes());
    if f.truthy() {
        f
    } else {
        s("undef")
    }
}

/// Add user-defined MIE groups to %mieMap
/// Inputs: none;  Returns: nothing, but sets $doneMieMap flag
pub fn update_mie_map(it: &mut Interp, _a: Vec<Value>, want: Want) -> ER<Vec<Value>> {
    put(&done_mie_map_var(it), n(1)); // set flag so we only do this once
    let user_defined = pl::pkg_hash(it, "Image::ExifTool::UserDefined");
    if user_defined.keys().is_empty() {
        return pl::ret_nothing(want);
    }
    let mie_map = mie_map(it);
    let mut tables: Vec<(Value, Value)> = Vec::new();
    let mut done_table: HashSet<Vec<u8>> = HashSet::new();
    // get list of top-level MIE tables with user-defined tags
    for table_name in keys(&user_defined) {
        if !re(r"^Image::ExifTool::MIE::", "").is_match(&table_name) {
            continue;
        }
        let user_table = user_defined.get(table_name.to_bytes());
        let tag_table_ptr = etf(it, "GetTagTable", vec![table_name.clone()])?;
        if !tag_table_ptr.truthy() {
            continue;
        }
        // copy the WRITE_GROUP from the actual table
        user_table.set("WRITE_GROUP", tag_table_ptr.get("WRITE_GROUP"));
        // add to list of tables to process
        done_table.insert(table_name.to_bytes().to_vec());
        tables.push((table_name, user_table));
    }
    // recursively add all user-defined groups to MIE map
    while !tables.is_empty() {
        let (table_name, tag_table_ptr) = tables.remove(0);
        let parent = tag_table_ptr.get("WRITE_GROUP");
        if !parent.truthy() {
            pl::warn(it, &format!("No WRITE_GROUP for {}\n", table_name.to_text()))?;
            continue;
        }
        if !mie_map.get(parent.to_bytes()).truthy() {
            pl::warn(it, &format!("{} is not in MIE map\n", parent.to_text()))?;
            continue;
        }
        let pmie = process_mie_ref(it);
        for tag_id in pl::call_list(it, "Image::ExifTool::TagTableKeys", vec![tag_table_ptr.clone()])? {
            let tag_info = tag_table_ptr.get(tag_id.to_bytes());
            if !(tag_info.is_hash() && tag_info.get("SubDirectory").truthy()) {
                continue;
            }
            let sub_table_name = tag_info.get("SubDirectory").get("TagTable");
            let sub_table_ptr = etf(it, "GetTagTable", vec![sub_table_name.clone()])?;
            if !sub_table_ptr.truthy() {
                continue;
            }
            // only care about MIE tables
            let pp = sub_table_ptr.get("PROCESS_PROC");
            if !(pp.truthy() && seq(&pp, &pmie)) {
                continue;
            }
            let group = sub_table_ptr.get("WRITE_GROUP");
            if !group.truthy() {
                pl::warn(it, &format!("No WRITE_GROUP for {}\n", sub_table_name.to_text()))?;
                continue;
            }
            let cur = mie_map.get(group.to_bytes());
            if cur.truthy() && !seq(&cur, &parent) {
                pl::warn(it, &format!("{} already has different parent ({})\n", group.to_text(), cur.to_text()))?;
                continue;
            }
            mie_map.set(group.to_bytes(), parent.clone()); // add to map
            // process tables within this one too
            if done_table.contains(&sub_table_name.to_bytes()[..]) {
                continue;
            }
            done_table.insert(sub_table_name.to_bytes().to_vec());
            tables.push((sub_table_name, sub_table_ptr));
        }
    }
    pl::ret_nothing(want)
}

/// Get localized version of tagInfo hash
/// Inputs: 0) tagInfo hash ref, 1) locale code (eg. "en_CA")
/// Returns: new tagInfo hash ref, or undef if invalid
pub fn get_lang_info(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (tag_info, mut lang_code) = (arg(&a, 0), arg(&a, 1));
    // check for properly formatted language code
    let Some(c) = re(r"^[a-z]{2}([-_])[A-Z]{2}$", "").m(&lang_code) else {
        return pl::ret_undef();
    };
    // use '_' as a separator, but recognize '_' or '-'
    if cap(&c, 1).eq_str("-") {
        pl::tr(&mut lang_code, "-", "_", "");
    }
    // can only set locale on string types
    let w = tag_info.get("Writable");
    if w.truthy() && !w.eq_str("string") {
        return pl::ret_undef();
    }
    ret(pl::call(it, "Image::ExifTool::GetLangInfo", vec![tag_info, lang_code])?)
}

/// `HasZlib($et, $verb)` from Rust.
fn has_zlib_rs(it: &mut Interp, et: &Value, verb: &str) -> ER<bool> {
    Ok(has_zlib(it, vec![et.clone(), s(verb)], Want::Scalar)?.pop().unwrap_or_default().truthy())
}

/// return true if we have Zlib::Compress
/// Inputs: 0) ExifTool object ref, 1) verb for what you want to do with the info
/// Returns: 1 if Zlib available, 0 otherwise
pub fn has_zlib(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let hz = has_zlib_var(it);
    if get(&hz).is_undef() {
        let ok = pl::eval_block(it, |it| crate::runtime::require(it, "Compress::Zlib"))?.is_some();
        put(&hz, if ok { n(1) } else { Value::Undef });
        if !get(&hz).truthy() {
            put(&hz, n(0));
            let msg = cat(&[&s("Install Compress::Zlib to "), &arg(&a, 1), &s(" compressed information")]);
            pl::method(it, &arg(&a, 0), "Warn", vec![msg])?;
        }
    }
    ret(get(&hz))
}

/// `MIEGroupFormat($chr)` from Rust.
fn mie_group_format_rs(it: &mut Interp, chr_: bool) -> ER<Value> {
    Ok(mie_group_format(it, if chr_ { vec![n(1)] } else { vec![] }, Want::Scalar)?.pop().unwrap_or_default())
}

/// Get format code for MIE group element with current byte order
/// Inputs: 0) [optional] true to convert result to chr()
/// Returns: format code
pub fn mie_group_format(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let chr_ = arg(&a, 0);
    let format = if byte_order(it)?.eq_str("MM") { n(0x10) } else { n(0x18) };
    ret(if chr_.truthy() { chr(&format) } else { format })
}

/// ReadValue() with added support for UTF formats (utf8, utf16 and utf32)
/// Inputs: 0) data reference, 1) value offset, 2) format string,
///         3) number of values (or undef to use all data)
///         4) valid data length relative to offset, 5) returned rational ref
/// Returns: converted value, or undefined if data isn't there
///          or list of values in list context
/// Notes: all string formats are converted to UTF8
pub fn read_mie_value(it: &mut Interp, a: Vec<Value>, want: Want) -> ER<Vec<Value>> {
    let (data_pt, offset, mut format, count, size) = (arg(&a, 0), arg(&a, 1), arg(&a, 2), arg(&a, 3), arg(&a, 4));
    let mut val;
    if let Some(c) = re(r"^(utf(8|16|32)|string)", "").m(&format) {
        let m1 = cap(&c, 1);
        if m1.eq_str("utf8") || m1.eq_str("string") {
            // read the 8-bit string
            val = pl::substr(&data_pt.deref(), offset.to_i64(), Some(size.to_i64()));
            // (as of ExifTool 7.62, leave string values unconverted)
        } else {
            // convert to UTF8
            let fmt = if byte_order(it)?.eq_str("MM") {
                if m1.eq_str("utf16") { "n" } else { "N" }
            } else if m1.eq_str("utf16") {
                "v"
            } else {
                "V"
            };
            let unpk = pl::unpack(&format!("x{}{}{}", offset.to_text(), fmt, size.to_text()), &data_pt.deref())?;
            val = pl::pack("C0U*", &unpk)?;
        }
        // truncate at null unless this is a list
        // (strings shouldn't have a null, but just in case)
        if !re(r"_list$", "").is_match(&format) {
            re(r"\0.*", "s").subst(&mut val, false, |_| Vec::new());
        }
    } else {
        if format.eq_str("free") {
            format = s("undef"); // read 'free' as 'undef'
        }
        let args = vec![data_pt, offset, format, count, size, arg(&a, 5)];
        return if want == Want::List { pl::call_list(it, "Image::ExifTool::ReadValue", args) } else { ret(pl::call(it, "Image::ExifTool::ReadValue", args)?) };
    }
    ret(val)
}

/// `ReadMIEValue(...)` in scalar context from Rust.
fn read_mie_value1(it: &mut Interp, args: Vec<Value>) -> ER<Value> {
    Ok(read_mie_value(it, args, Want::Scalar)?.pop().unwrap_or_default())
}

/// validate raw values for writing
/// Inputs: 0) ExifTool object ref, 1) tagInfo hash ref, 2) raw value ref
/// Returns: error string or undef (and possibly changes value) on success
pub fn check_mie(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, tag_info, val_ptr) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    let w = tag_info.get("Writable");
    let format = if w.truthy() { w } else { tag_info.get("Table").get("WRITABLE") };
    let err;

    if !format.truthy() || format.eq_str("1") {
        return ret(s("No writable format"));
    }
    // handle units if supported by this tag
    let ulist = tag_info.get("Units");
    let m = if ulist.truthy() { re(r"(.*)\((.*)\)$", "").m(&val_ptr.deref()) } else { None };
    if let Some(c) = m {
        let (val, units) = (cap(&c, 1), cap(&c, 2));
        let pat = [&b"^"[..], &units.to_bytes(), b"$"].concat();
        let r = pl::re_dyn(&pat, "i")?;
        let units = ulist.items().into_iter().find(|u| r.is_match(u));
        let Some(units) = units else {
            return ret(cat(&[&s("Allowed units: ("), &pl::join("|", &ulist.items()), &s(")")]));
        };
        let vc = cell(val);
        err = pl::call(it, "Image::ExifTool::CheckValue", vec![Value::ScalarRef(vc.clone()), format.clone(), tag_info.get("Count")])?;
        // add units back onto value
        if !err.truthy() {
            pl::set_deref(&val_ptr, cat(&[&get(&vc), &s("("), &units, &s(")")]))?;
        }
    } else if !re(r"^(utf|string|undef)", "").is_match(&format) && re(r"\)$", "").is_match(&val_ptr.deref()) {
        return ret(s("Units not supported"));
    } else {
        if format.eq_str("string") && !et.get("OPTIONS").get("Charset").eq_str("UTF8") && re(r"[\x80-\xff]", "").is_match(&val_ptr.deref()) {
            // convert from Charset to UTF-8
            let v = pl::method(it, &et, "Encode", vec![val_ptr.deref(), s("UTF8")])?;
            pl::set_deref(&val_ptr, v)?;
        }
        err = pl::call(it, "Image::ExifTool::CheckValue", vec![val_ptr.clone(), format.clone(), tag_info.get("Count")])?;
    }
    ret(err)
}

/// `WriteMIEGroup(...)` from Rust.
fn write_mie_group_rs(it: &mut Interp, et: &Value, dir_info: Value, table: &Value) -> ER<Value> {
    Ok(write_mie_group(it, vec![et.clone(), dir_info, table.clone()], Want::Scalar)?.pop().unwrap_or_default())
}

/// Rewrite a MIE directory
/// Inputs: 0) ExifTool object reference, 1) DirInfo reference, 2) tag table ptr
/// Returns: undef on success, otherwise error message (empty message if nothing to write)
/// (the `my` variables start undef as in the Perl, hence the unused assignments)
#[allow(unused_assignments)]
pub fn write_mie_group(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info, tag_table_ptr) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    let outfile = dir_info.get("OutFile");
    let dir_name = dir_info.get("DirName");
    let tw = dir_info.get("ToWrite");
    let mut to_write = if tw.truthy() { tw } else { s("") };
    let mut raf = dir_info.get("RAF");
    let verbose = opt(it, &et, "Verbose")?;
    let opt_compress = opt(it, &et, "Compress")?;
    let out = opt(it, &et, "TextOut")?;
    let (mut msg, mut err, mut ok, mut sync, mut del_group) = (Value::Undef, false, false, Value::Undef, false);
    let mut tag = s("");
    let mut deleted_tag = s("");

    // count each MIE directory found and make name for this specific instance
    let mut is_writing: HashSet<Vec<u8>> = HashSet::new();
    let cnt = et.get("MIE_COUNT");
    let grp = tag_table_ptr.get("GROUPS").get("1");
    let nv = cnt.get("MIE-Main");
    let mut nn = if nv.truthy() { nv } else { n(0) };
    let mut grp1;
    if grp.eq_str("MIE-Main") {
        nn = pl::add(&nn, &n(1));
        cnt.set(grp.to_bytes(), nn.clone());
        grp1 = grp.clone();
        let rep = [&b"MIE"[..], &nn.to_bytes(), b"-"].concat();
        re(r"MIE-", "").subst(&mut grp1, false, |_| rep.clone());
    } else {
        grp1 = grp.clone();
        let rep = [&b"MIE"[..], &nn.to_bytes(), b"-"].concat();
        re(r"MIE-", "").subst(&mut grp1, false, |_| rep.clone());
        let c = cnt.get(grp1.to_bytes());
        let m = pl::add(&(if c.truthy() { c } else { n(0) }), &n(1));
        cnt.set(grp1.to_bytes(), m.clone());
        is_writing.insert(cat(&[&grp, &m]).to_bytes().to_vec()); // eg. 'MIE-Doc2'
        is_writing.insert(grp1.to_bytes().to_vec()); // eg. 'MIE1-Doc'
        grp1 = cat(&[&grp1, &m]);
    }
    // build lookup for all valid group names for this MIE group
    is_writing.insert(grp.to_bytes().to_vec()); // eg. 'MIE-Doc'
    is_writing.insert(grp1.to_bytes().to_vec()); // eg. 'MIE1-Doc2'
    is_writing.insert(cat(&[&s("MIE"), &nn]).to_bytes().to_vec()); // eg. 'MIE1'

    // determine if we are deleting this group
    let dg = et.get("DEL_GROUP");
    if !dg.keys().is_empty() && (dg.get("MIE").truthy() || dg.get(grp.to_bytes()).truthy() || dg.get(grp1.to_bytes()).truthy() || dg.get(cat(&[&s("MIE"), &nn]).to_bytes()).truthy()) {
        del_group = true;
    }

    // prepare lookups and lists for writing
    let new_tags = pl::method(it, &et, "GetNewTagInfoHash", vec![tag_table_ptr.clone()])?;
    let ad = pl::method_list(it, &et, "GetAddDirHash", vec![tag_table_ptr.clone(), dir_name.clone()])?;
    let (add_dirs, edit_dirs) = (arg(&ad, 0), arg(&ad, 1));
    let mut edit_tags = keys(&new_tags);
    edit_tags.extend(keys(&edit_dirs));
    edit_tags.sort_by(ops::str_cmp);
    if verbose.truthy() {
        let w = if raf.truthy() { s("Writing") } else { s("Creating") };
        print_out(it, &out, &[w, cat(&[&s(" "), &grp1, &s(":\n")])])?;
    }

    // loop through elements in MIE group
    'mie_element: loop {
        let (mut format, mut tag_len, mut val_len, mut units, mut old_hdr, mut buff) = (Value::Undef, Value::Undef, Value::Undef, Value::Undef, Value::Undef, Value::Undef);
        let last_tag = tag.clone();
        if raf.truthy() {
            // read first 4 bytes of element header
            let nr = raf_read(it, &raf, &mut old_hdr, n(4))?;
            if !pl::num_eq(&nr, &n(4)) {
                if nr.truthy() || !sync.is_undef() {
                    break;
                }
                raf = Value::Undef; // all done reading
                ok = true;
            }
        }
        if raf.truthy() {
            let u = pl::unpack("aC3", &old_hdr)?;
            sync = arg(&u, 0);
            format = arg(&u, 1);
            tag_len = arg(&u, 2);
            val_len = arg(&u, 3);
            if !sync.eq_str("~") {
                msg = s("Invalid sync byte");
                break;
            }

            // read tag name
            if tag_len.truthy() {
                if !raf_read_eq(it, &raf, &mut tag, &tag_len)? {
                    break;
                }
                old_hdr = pl::concat(&old_hdr, &tag); // add tag to element header
                if slt(&tag, &last_tag) {
                    warn(it, &et, cat(&[&s("MIE tag '"), &tag, &s("' out of sequence")]))?;
                }
                // separate units from tag name if they exist
                let mut u1 = Value::Undef;
                if re(r"\((.*)\)$", "").subst(&mut tag, false, |c| {
                    u1 = cap(c, 1);
                    Vec::new()
                }) > 0
                {
                    units = u1;
                }
            } else {
                tag = s("");
            }

            // get multi-byte value length if necessary
            if pl::gt(&val_len, &n(252)) {
                // calculate number of bytes in extended DataLength
                let nb = ops::shift_left(&n(1), &pl::sub(&n(256), &val_len));
                if !raf_read_eq(it, &raf, &mut buff, &nb)? {
                    break;
                }
                old_hdr = pl::concat(&old_hdr, &buff); // add to old header
                let fmt = cat(&[&s("int"), &pl::mul(&nb, &n(8)), &s("u")]);
                val_len = read_value(it, vec![pl::sref(buff.clone()), n(0), fmt, n(1), nb.clone()])?;
                if pl::gt(&val_len, &n(0x7fffffff)) {
                    msg = cat(&[&s("Can't write "), &tag, &s(" (DataLength > 2GB not yet supported)")]);
                    break;
                }
            }
            // don't rewrite free bytes or information in deleted groups
            if pl::num_eq(&format, &n(0x80)) || (del_group && tag_len.truthy() && !pl::num_eq(&band(&format, 0xf0), &n(0x10))) {
                if !seek(it, &raf, val_len.clone(), 1)? {
                    msg = s("Seek error");
                    break;
                }
                if pl::gt(&verbose, &n(1)) {
                    let free = if pl::num_eq(&format, &n(0x80)) { s(" free") } else { s("") };
                    print_out(it, &out, &[cat(&[&s("    - "), &grp1, &s(":"), &tag, &s(" ("), &val_len, &free, &s(" bytes)\n")])])?;
                }
                if del_group {
                    inc(&et, "CHANGED");
                }
                continue;
            }
        } else {
            // no more elements to read
            tag_len = n(0);
            val_len = n(0);
            tag = s("");
        }
        //
        // write necessary new tags and process directories
        //
        while !edit_tags.is_empty() {
            if tag_len.truthy() && sgt(&edit_tags[0], &tag) {
                break;
            }
            // we are writing the new tag now
            let new_val = cell(Value::Undef);
            let (mut writable, mut old_val, mut new_format, mut compress) = (Value::Undef, Value::Undef, Value::Undef, false);
            let mut new_tag = edit_tags.remove(0);
            if len(&new_tag).to_i64() > 255 {
                warn(it, &et, s("Tag name too long"))?;
                continue; // (just to be safe)
            }
            let mut new_info = edit_dirs.get(new_tag.to_bytes());
            if new_info.truthy() {
                // create the new subdirectory or rewrite existing non-MIE directory
                let sub_table_ptr = etf(it, "GetTagTable", vec![new_info.get("SubDirectory").get("TagTable")])?;
                if !sub_table_ptr.truthy() {
                    warn(it, &et, cat(&[&s("No tag table for "), &new_tag, &s(" "), &new_info.get("Name")]))?;
                    continue;
                }
                let mut subdir_info = Value::new_hash();
                let wp = sub_table_ptr.get("WRITE_PROC");
                let pmie = process_mie_ref(it);
                let is_mie_group = wp.truthy() && seq(&wp, &pmie);

                if seq(&new_tag, &tag) {
                    // make sure that either both or neither old and new tags are MIE groups
                    if is_mie_group ^ pl::num_eq(&band(&format, 0xf3), &n(0x10)) {
                        warn(it, &et, cat(&[&s("Tag '"), &tag, &s("' not expected type")]))?;
                        continue; // don't write our new tag
                    }
                    // uncompress existing directory into $oldVal since we are editing it
                    if band(&format, 0x04).truthy() {
                        if !has_zlib_rs(it, &et, "edit")? {
                            break;
                        }
                        if !raf_read_eq(it, &raf, &mut old_val, &val_len)? {
                            break 'mie_element;
                        }
                        let mut stat = Value::Undef;
                        let inflate = pl::call(it, "Compress::Zlib::inflateInit", vec![])?;
                        if inflate.truthy() {
                            let mut inp = old_val.clone();
                            let (o, st) = inflate_call(it, &inflate, &mut inp)?;
                            old_val = o;
                            stat = st;
                        }
                        if !(inflate.truthy() && pl::num_eq(&stat, &z_stream_end(it)?)) {
                            msg = cat(&[&s("Error inflating "), &tag]);
                            break 'mie_element;
                        }
                        compress = true;
                        val_len = len(&old_val); // uncompressed value length
                    }
                } else {
                    // don't create this directory unless necessary
                    if !add_dirs.get(new_tag.to_bytes()).truthy() {
                        continue;
                    }
                }

                if is_mie_group {
                    if seq(&new_tag, &tag) {
                        // rewrite existing directory later unless it was compressed
                        if !compress {
                            break;
                        }
                        // rewrite directory to '$newVal'
                        put(&new_val, s(""));
                        subdir_info.set("OutFile", Value::ScalarRef(new_val.clone()));
                        let r = pl::method(it, &s("File::RandomAccess"), "new", vec![pl::sref(old_val.clone())])?;
                        subdir_info.set("RAF", r);
                    } else if opt_compress.truthy() && !dir_info.get("IsCompressed").truthy() {
                        // write to memory so we can compress the new MIE group
                        compress = true;
                        subdir_info.set("OutFile", Value::ScalarRef(new_val.clone()));
                    } else {
                        let gf = mie_group_format_rs(it, true)?;
                        let hdr = cat(&[&s("~"), &gf, &chr(&len(&new_tag)), &s("\0"), &new_tag]);
                        subdir_info.set("OutFile", outfile.clone());
                        subdir_info.set("ToWrite", pl::concat(&to_write, &hdr));
                    }
                    let dn = new_info.get("SubDirectory").get("DirName");
                    subdir_info.set("DirName", if dn.truthy() { dn } else { new_tag.clone() });
                    subdir_info.set("Parent", dir_name.clone());
                    // don't compress elements of an already compressed group
                    let ic = dir_info.get("IsCompressed");
                    subdir_info.set("IsCompressed", if ic.truthy() { ic } else if compress { n(1) } else { Value::Undef });
                    msg = write_mie_group_rs(it, &et, subdir_info, &sub_table_ptr)?;
                    if msg.truthy() {
                        break 'mie_element;
                    }
                    // message is defined but empty if nothing was written
                    if !msg.is_undef() {
                        msg = Value::Undef; // not a problem if nothing was written
                        continue;
                    } else if !compress {
                        // group was written already
                        to_write = s("");
                        continue;
                    } else if len(&get(&new_val)).to_i64() <= 4 {
                        // terminator only?
                        if verbose.truthy() {
                            print_out(it, &out, &[cat(&[&s("Deleted compressed "), &grp1, &s(" (empty)\n")])])?;
                        }
                        if seq(&new_tag, &tag) {
                            continue 'mie_element; // deleting the directory
                        }
                        continue; // not creating the new directory
                    }
                    writable = s("undef");
                    new_format = mie_group_format_rs(it, false)?;
                } else {
                    if seq(&new_tag, &tag) {
                        if !compress {
                            // read and edit existing directory
                            if !raf_read_eq(it, &raf, &mut old_val, &val_len)? {
                                break 'mie_element;
                            }
                        }
                        subdir_info = Value::new_hash();
                        subdir_info.set("DataPt", pl::sref(old_val.clone()));
                        subdir_info.set("DataLen", val_len.clone());
                        subdir_info.set("DirName", new_info.get("Name"));
                        let dp = if dir_info.get("IsCompressed").truthy() { Value::Undef } else { pl::sub(&tell(it, &raf)?, &val_len) };
                        subdir_info.set("DataPos", dp);
                        subdir_info.set("DirStart", n(0));
                        subdir_info.set("DirLen", val_len.clone());
                        // write Compact subdirectories if we will compress the data
                        if (compress || opt_compress.truthy() || dir_info.get("IsCompressed").truthy()) && pl::eval_block(it, |it| crate::runtime::require(it, "Compress::Zlib"))?.is_some() {
                            subdir_info.set("Compact", n(1));
                            subdir_info.set("ReadOnly", n(1)); // because XMP is not writable in place
                        }
                    }
                    subdir_info.set("Parent", dir_name.clone());
                    let write_proc = new_info.get("SubDirectory").get("WriteProc");
                    // reset processed lookup to avoid errors in case of multiple EXIF blocks
                    et.set("PROCESSED", Value::new_hash());
                    let nvv = pl::method(it, &et, "WriteDirectory", vec![subdir_info, sub_table_ptr.clone(), write_proc])?;
                    put(&new_val, nvv.clone());
                    if !nvv.is_undef() {
                        if nvv.eq_str("") {
                            if seq(&new_tag, &tag) {
                                continue 'mie_element; // deleting the directory
                            }
                            continue; // not creating the new directory
                        }
                    } else {
                        if old_val.is_undef() {
                            continue;
                        }
                        put(&new_val, old_val.clone()); // just copy over the old directory
                    }
                    writable = s("undef");
                    new_format = n(0x00); // all other directories are 'undef' format
                }
            } else {
                // get the new tag information
                new_info = new_tags.get(new_tag.to_bytes());
                let nv_hash = pl::method(it, &et, "GetNewValueHash", vec![new_info.clone()])?;
                let mut new_vals: Vec<Value> = Vec::new();

                // write information only to specified group
                let write_group = nv_hash.get("WriteGroup");
                if !is_writing.contains(&write_group.to_bytes()[..]) {
                    break;
                }

                // if tag existed, must decide if we want to overwrite the value
                if seq(&new_tag, &tag) {
                    let mut is_overwriting;
                    let is_list = new_info.get("List");
                    if is_list.truthy() {
                        if nv_hash.get("CreateOnly").truthy() {
                            break;
                        }
                        is_overwriting = n(-1); // force processing list elements individually
                    } else {
                        is_overwriting = pl::method(it, &et, "IsOverwriting", vec![nv_hash.clone()])?;
                        if !is_overwriting.truthy() {
                            break;
                        }
                    }
                    let (mut val, mut cmp_val) = (Value::Undef, Value::Undef);
                    if pl::lt(&is_overwriting, &n(0)) || pl::gt(&verbose, &n(1)) {
                        // check to be sure we can uncompress the value if necessary
                        if band(&format, 0x04).truthy() && !has_zlib_rs(it, &et, "edit")? {
                            break;
                        }
                        // read the old value
                        if !raf_read_eq(it, &raf, &mut old_val, &val_len)? {
                            break 'mie_element;
                        }
                        // uncompress if necessary
                        if band(&format, 0x04).truthy() {
                            let mut stat = Value::Undef;
                            let inflate = pl::call(it, "Compress::Zlib::inflateInit", vec![])?;
                            // must save original compressed value in case we decide
                            // not to overwrite it later
                            cmp_val = old_val.clone();
                            if inflate.truthy() {
                                let mut inp = old_val.clone();
                                let (o, st) = inflate_call(it, &inflate, &mut inp)?;
                                old_val = o;
                                stat = st;
                            }
                            if !(inflate.truthy() && pl::num_eq(&stat, &z_stream_end(it)?)) {
                                msg = cat(&[&s("Error inflating "), &tag]);
                                break 'mie_element;
                            }
                            val_len = len(&old_val); // update value length
                        }
                        // convert according to specified format
                        let format_str = format_str_of(it, &format);
                        val = read_mie_value1(it, vec![pl::sref(old_val.clone()), n(0), format_str.clone(), Value::Undef, val_len.clone()])?;
                        if pl::lt(&is_overwriting, &n(0)) && !val.is_undef() {
                            // handle list values individually
                            if is_list.truthy() {
                                let vals = if re(r"_list$", "").is_match(&format_str) { pl::split(&re(r"\0", ""), &val, 0) } else { vec![val.clone()] };
                                // keep any list items that we aren't overwriting
                                for v in vals {
                                    if pl::method(it, &et, "IsOverwriting", vec![nv_hash.clone(), v.clone()])?.truthy() {
                                        continue;
                                    }
                                    new_vals.push(v);
                                }
                            } else {
                                // test to see if we really want to overwrite the value
                                is_overwriting = pl::method(it, &et, "IsOverwriting", vec![nv_hash.clone(), val.clone()])?;
                            }
                        }
                    }
                    if is_overwriting.truthy() {
                        // skip the old value if we didn't read it already
                        if old_val.is_undef() && !seek(it, &raf, val_len.clone(), 1)? {
                            msg = s("Seek error");
                        }
                        if pl::gt(&verbose, &n(1)) {
                            if !units.is_undef() {
                                val = cat(&[&val, &s("("), &units, &s(")")]);
                            }
                            pl::method(it, &et, "VerboseValue", vec![cat(&[&s("- "), &grp1, &s(":"), &new_info.get("Name")]), val.clone()])?;
                        }
                        deleted_tag = tag.clone(); // remember that we deleted this tag
                        inc(&et, "CHANGED"); // we deleted the old value
                    } else {
                        if !old_val.is_undef() {
                            // write original compressed value
                            if !cmp_val.is_undef() {
                                old_val = cmp_val.clone();
                            }
                        } else if !raf_read_eq(it, &raf, &mut old_val, &val_len)? {
                            break 'mie_element;
                        }
                        // write the old value now
                        if !write(it, &outfile, vec![to_write.clone(), old_hdr.clone(), old_val.clone()])? {
                            err = true;
                        }
                        to_write = s("");
                        continue 'mie_element;
                    }
                    if new_vals.is_empty() {
                        // unshift the new tag info to write it later
                        edit_tags.insert(0, new_tag);
                        continue 'mie_element; // get next element from file
                    }
                } else {
                    // write new value if creating, or if List and list existed, or
                    // if tag was previously deleted
                    if !(nv_hash.get("IsCreating").truthy() || (seq(&new_tag, &last_tag) && (new_info.get("List").truthy() || seq(&deleted_tag, &last_tag)))) {
                        continue;
                    }
                }
                // get the new value to write (undef to delete)
                new_vals.extend(pl::method_list(it, &et, "GetNewValue", vec![nv_hash.clone()])?);
                if new_vals.is_empty() {
                    continue;
                }
                let w = new_info.get("Writable");
                writable = if w.truthy() { w } else { tag_table_ptr.get("WRITABLE") };
                if writable.eq_str("string") {
                    // join multiple values into a single string
                    put(&new_val, pl::join("\0", &new_vals));
                    // write string as UTF-8,16 or 32 if value contains valid UTF-8 codes
                    let is_utf8 = pl::call(it, "Image::ExifTool::IsUTF8", vec![Value::ScalarRef(new_val.clone())])?;
                    if pl::gt(&is_utf8, &n(0)) {
                        writable = s("utf8");
                        // write UTF-16 or UTF-32 if it is more compact
                        let to = if pl::gt(&is_utf8, &n(1)) { s("UCS4") } else { s("UTF16") };
                        let tmp = pl::call(it, "Image::ExifTool::Decode", vec![Value::Undef, get(&new_val), s("UTF8"), Value::Undef, to])?;
                        if len(&tmp).to_i64() < len(&get(&new_val)).to_i64() {
                            put(&new_val, tmp);
                            writable = if pl::gt(&is_utf8, &n(1)) { s("utf32") } else { s("utf16") };
                        }
                    }
                    // write as a list if we have multiple values
                    if new_vals.len() > 1 {
                        writable = pl::concat(&writable, &s("_list"));
                    }
                } else {
                    // should only be one element in the list
                    put(&new_val, new_vals.remove(0));
                }
                new_format = mie_code(it).get(writable.to_bytes());
                if new_format.is_undef() {
                    msg = cat(&[&s("Bad format '"), &writable, &s("' for "), &new_info.get("Name")]);
                    continue 'mie_element;
                }
            }

            // write the new or edited element
            #[allow(clippy::never_loop)]
            while !new_format.is_undef() {
                let mut val_pt: Cell = new_val.clone();
                // remove units from value and add to tag name if supported by this tag
                if new_info.get("Units").truthy() {
                    let val2;
                    if let Some(c) = re(r"(.*)\((.*)\)$", "").m(&get(&val_pt)) {
                        val2 = cap(&c, 1);
                        new_tag = cat(&[&new_tag, &s("("), &cap(&c, 2), &s(")")]);
                    } else {
                        val2 = get(&val_pt);
                        // add default units
                        let ustr = cat(&[&s("("), &new_info.get("Units").at(0), &s(")")]);
                        new_tag = pl::concat(&new_tag, &ustr);
                        let v = pl::concat(&get(&val_pt), &ustr);
                        put(&val_pt, v);
                    }
                    val_pt = cell(val2);
                }
                // convert value if necessary
                if !re(r"^(utf|string|undef)", "").is_match(&writable) {
                    let val3 = write_value(it, vec![get(&val_pt), writable.clone(), new_info.get("Count")])?;
                    if val3.is_undef() {
                        warn(it, &et, cat(&[&s("Error writing "), &new_tag]))?;
                        break;
                    }
                    val_pt = cell(val3);
                }
                let mut l = len(&get(&val_pt));
                // compress value before writing if required
                if (compress || opt_compress.truthy()) && !dir_info.get("IsCompressed").truthy() && has_zlib_rs(it, &et, "write")? {
                    let deflate = pl::call(it, "Compress::Zlib::deflateInit", vec![])?;
                    let mut val4 = Value::Undef;
                    if deflate.truthy() {
                        val4 = pl::method(it, &deflate, "deflate", vec![get(&val_pt)])?;
                        if !val4.is_undef() {
                            let f = pl::method(it, &deflate, "flush", vec![])?;
                            val4 = pl::concat(&val4, &f);
                        }
                    }
                    if !val4.is_undef() {
                        let len4 = len(&val4);
                        let saved = pl::sub(&l, &len4);
                        // only use compressed data if it is smaller
                        if pl::gt(&saved, &n(0)) {
                            if verbose.truthy() {
                                print_out(it, &out, &[cat(&[&s("  ["), &new_tag, &s(" compression saved "), &saved, &s(" bytes]\n")])])?;
                            }
                            new_format = ops::bit_or(&new_format, &n(0x04)); // set compressed bit
                            l = len4; // set length
                            val_pt = cell(val4); // set value pointer
                        } else if verbose.truthy() {
                            print_out(it, &out, &[cat(&[&s("  ["), &new_tag, &s(" compression saved "), &saved, &s(" bytes -- written uncompressed]\n")])])?;
                        }
                    } else {
                        warn(it, &et, cat(&[&s("Error deflating "), &new_tag, &s(" (written uncompressed)")]))?;
                    }
                }
                // calculate the DataLength code
                let ext_len;
                if pl::lt(&l, &n(253)) {
                    ext_len = s("");
                } else if pl::lt(&l, &n(65536)) {
                    ext_len = s16u(it, l.clone())?;
                    l = n(255);
                } else if pl::le(&l, &n(0x7fffffff)) {
                    ext_len = s32u(it, l.clone())?;
                    l = n(254);
                } else {
                    warn(it, &et, cat(&[&s("Can't write "), &new_tag, &s(" (DataLength > 2GB not yet supported)")]))?;
                    break; // don't write this tag
                }
                // write this element (with leading MIE group element if not done already)
                let hdr = cat(&[&to_write, &s("~"), &chr(&new_format), &chr(&len(&new_tag))]);
                if !write(it, &outfile, vec![hdr, chr(&l), new_tag.clone(), ext_len, get(&val_pt)])? {
                    err = true;
                }
                to_write = s("");
                // we changed a tag unless just editing a subdirectory
                if !edit_dirs.get(new_tag.to_bytes()).truthy() {
                    pl::method(it, &et, "VerboseValue", vec![cat(&[&s("+ "), &grp1, &s(":"), &new_info.get("Name")]), get(&new_val)])?;
                    inc(&et, "CHANGED");
                }
                break; // didn't want to loop anyway
            }
            if !old_val.is_undef() {
                continue 'mie_element;
            }
        }
        //
        // rewrite existing element or descend into uncompressed MIE group
        //
        // all done this MIE group if we reached the terminator element
        if !tag_len.truthy() {
            // skip over existing terminator data (if any)
            if val_len.truthy() && !seek(it, &raf, val_len.clone(), 1)? {
                break;
            }
            ok = true;
            // write group terminator if necessary
            if !to_write.truthy() {
                // write end-of-group terminator element
                let mut term = s("~\0\0\0");
                if !dir_info.get("Parent").truthy() {
                    // write extended terminator for file-level group
                    let mut l = out_pos(it, &outfile)?;
                    // include length of terminator itself minus original $outfile position
                    let op = dir_info.get("OutPos");
                    l = pl::add(&l, &pl::sub(&n(10), &(if op.truthy() { op } else { n(0) })));
                    if l.truthy() && pl::le(&l, &n(0x7fffffff)) {
                        let s32 = s32u(it, l)?;
                        let gf = mie_group_format_rs(it, true)?;
                        term = cat(&[&s("~\0\0\x06"), &s32, &gf, &s("\x04")]);
                    }
                }
                if !write(it, &outfile, vec![term])? {
                    err = true;
                }
            }
            break;
        }

        // descend into existing uncompressed MIE group
        if pl::num_eq(&format, &n(0x10)) || pl::num_eq(&format, &n(0x18)) {
            let mut sub_dir_name = Value::Undef;
            let tag_info = pl::method(it, &et, "GetTagInfo", vec![tag_table_ptr.clone(), tag.clone()])?;
            let sub_table_ptr = if tag_info.truthy() && tag_info.get("SubDirectory").truthy() {
                sub_dir_name = tag_info.get("SubDirectory").get("DirName");
                let sub_table = tag_info.get("SubDirectory").get("TagTable");
                if sub_table.truthy() {
                    etf(it, "GetTagTable", vec![sub_table])?
                } else {
                    tag_table_ptr.clone()
                }
            } else {
                etf(it, "GetTagTable", vec![s("Image::ExifTool::MIE::Unknown")])?
            };
            let hdr = cat(&[&s("~"), &chr(&format), &chr(&len(&tag)), &s("\0"), &tag]);
            let subdir_info = Value::new_hash();
            subdir_info.set("DirName", if sub_dir_name.truthy() { sub_dir_name.clone() } else { tag.clone() });
            subdir_info.set("RAF", raf.clone());
            subdir_info.set("ToWrite", pl::concat(&to_write, &hdr));
            subdir_info.set("OutFile", outfile.clone());
            subdir_info.set("Parent", sub_dir_name.clone());
            subdir_info.set("IsCompressed", dir_info.get("IsCompressed"));
            let old_order = byte_order(it)?;
            set_byte_order(it, if band(&format, 0x08).truthy() { s("II") } else { s("MM") })?;
            msg = write_mie_group_rs(it, &et, subdir_info, &sub_table_ptr)?;
            set_byte_order(it, old_order)?;
            if msg.truthy() {
                break;
            }
            if !msg.is_undef() {
                msg = Value::Undef; // no problem if nothing written
            } else {
                to_write = s("");
            }
            continue;
        }
        // just copy existing element
        let mut old_val = Value::Undef;
        if !raf_read_eq(it, &raf, &mut old_val, &val_len)? {
            break;
        }
        if to_write.truthy() {
            if !write(it, &outfile, vec![to_write.clone()])? {
                err = true;
            }
            to_write = s("");
        }
        if !write(it, &outfile, vec![old_hdr.clone(), old_val])? {
            err = true;
        }
    }
    // return error message
    if err {
        msg = s("Error writing file");
    } else if !ok && !msg.truthy() {
        msg = s("Unexpected end of file");
    } else if !msg.truthy() && to_write.truthy() {
        msg = s(""); // flag for nothing written
        if verbose.truthy() {
            print_out(it, &out, &[cat(&[&s("Deleted "), &grp1, &s(" (empty)\n")])])?;
        }
    }
    ret(msg)
}

/// `ProcessMIEGroup(...)` from Rust.
fn process_mie_group_rs(it: &mut Interp, et: &Value, dir_info: Value, table: &Value) -> ER<Value> {
    Ok(process_mie_group(it, vec![et.clone(), dir_info, table.clone()], Want::Scalar)?.pop().unwrap_or_default())
}

/// Process MIE directory
/// Inputs: 0) ExifTool object reference, 1) DirInfo reference, 2) tag table ref
/// Returns: undef on success, or error message if there was a problem
/// Notes: file pointer is positioned at the MIE end on entry
pub fn process_mie_group(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info, tag_table_ptr) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    let raf = dir_info.get("RAF");
    let verbose = opt(it, &et, "Verbose")?;
    let out = opt(it, &et, "TextOut")?;
    let not_utf8 = !et.get("OPTIONS").get("Charset").eq_str("UTF8");
    let (mut msg, mut buff, mut ok, mut old_indent, mut mime) = (Value::Undef, Value::Undef, false, Value::Undef, Value::Undef);
    let mut last_tag = s("");

    // get group 1 names: $grp doesn't have numbers (eg. 'MIE-Doc'),
    // and $grp1 does (eg. 'MIE1-Doc1')
    let cnt = et.get("MIE_COUNT");
    let mut grp1 = tag_table_ptr.get("GROUPS").get("1");
    let nv = cnt.get("MIE-Main");
    let mut nn = if nv.truthy() { nv } else { n(0) };
    let rep_mie = |grp1: &mut Value, nn: &Value| {
        let rep = [&b"MIE"[..], &nn.to_bytes(), b"-"].concat();
        re(r"MIE-", "").subst(grp1, false, |_| rep.clone());
    };
    if grp1.eq_str("MIE-Main") {
        nn = pl::add(&nn, &n(1));
        cnt.set(grp1.to_bytes(), nn.clone());
        if pl::gt(&nn, &n(1)) {
            rep_mie(&mut grp1, &nn);
        }
    } else {
        if pl::gt(&nn, &n(1)) {
            rep_mie(&mut grp1, &nn);
        }
        let c = cnt.get(grp1.to_bytes());
        cnt.set(grp1.to_bytes(), pl::add(&(if c.truthy() { c } else { n(0) }), &n(1)));
        let c = cnt.get(grp1.to_bytes());
        if pl::gt(&c, &n(1)) {
            grp1 = pl::concat(&grp1, &c);
        }
    }
    // set group1 name for all tags extracted from this group
    et.set("SET_GROUP1", grp1.clone());

    if verbose.truthy() {
        old_indent = et.get("INDENT");
        et.set("INDENT", pl::concat(&et.get("INDENT"), &s("| ")));
        pl::method(it, &et, "VerboseDir", vec![grp1.clone()])?;
    }
    let mut was_compressed = dir_info.get("WasCompressed");

    // process all MIE elements
    loop {
        if !raf_read_eq(it, &raf, &mut buff, &n(4))? {
            break;
        }
        let u = pl::unpack("aC3", &buff)?;
        let (sync, format, tag_len, mut val_len) = (arg(&u, 0), arg(&u, 1), arg(&u, 2), arg(&u, 3));
        if !sync.eq_str("~") {
            msg = s("Invalid sync byte");
            break;
        }

        // read tag name
        let (mut tag, mut units) = (Value::Undef, Value::Undef);
        if tag_len.truthy() {
            if !raf_read_eq(it, &raf, &mut tag, &tag_len)? {
                break;
            }
            if slt(&tag, &last_tag) {
                warn(it, &et, cat(&[&s("MIE tag '"), &tag, &s("' out of sequence")]))?;
            }
            last_tag = tag.clone();
            // separate units from tag name if they exist
            let mut u1 = Value::Undef;
            if re(r"\((.*)\)$", "").subst(&mut tag, false, |c| {
                u1 = cap(c, 1);
                Vec::new()
            }) > 0
            {
                units = u1;
            }
        } else {
            tag = s("");
        }

        // get multi-byte value length if necessary
        if pl::gt(&val_len, &n(252)) {
            let nb = ops::shift_left(&n(1), &pl::sub(&n(256), &val_len));
            if !raf_read_eq(it, &raf, &mut buff, &nb)? {
                break;
            }
            let fmt = cat(&[&s("int"), &pl::mul(&nb, &n(8)), &s("u")]);
            val_len = read_value(it, vec![pl::sref(buff.clone()), n(0), fmt, n(1), nb.clone()])?;
            if pl::gt(&val_len, &n(0x7fffffff)) {
                msg = cat(&[&s("Can't read "), &tag, &s(" (DataLength > 2GB not yet supported)")]);
                break;
            }
        }

        // all done if we reached the group terminator
        if !tag_len.truthy() {
            // skip over terminator data block
            if !(val_len.truthy() && !seek(it, &raf, val_len.clone(), 1)?) {
                ok = true;
            }
            break;
        }

        // get tag information hash unless this is free space
        let (mut tag_info, mut value) = (Value::Undef, Value::Undef);
        #[allow(clippy::never_loop)]
        while !pl::num_eq(&format, &n(0x80)) {
            tag_info = pl::method(it, &et, "GetTagInfo", vec![tag_table_ptr.clone(), tag.clone()])?;
            if tag_info.truthy() {
                break;
            }
            // extract tags with locale code
            if re(r"\W", "").is_match(&tag) {
                if let Some(c) = re(r"^(\w+)-([a-z]{2}_[A-Z]{2})$", "").m(&tag) {
                    let (base_tag, lang_code) = (cap(&c, 1), cap(&c, 2));
                    tag_info = pl::method(it, &et, "GetTagInfo", vec![tag_table_ptr.clone(), base_tag])?;
                    if tag_info.truthy() {
                        tag_info = get_lang_info(it, vec![tag_info, lang_code], Want::Scalar)?.pop().unwrap_or_default();
                    }
                    if tag_info.truthy() {
                        break;
                    }
                } else {
                    warn(it, &et, s("Invalid MIE tag name"))?;
                    break;
                }
            }
            // extract unknown tags if specified
            tag_info = Value::new_hash();
            tag_info.set("Name", tag.clone());
            tag_info.set("Writable", n(0));
            tag_info.set("PrintConv", Value::Code(it.sub_ref("Image::ExifTool::LimitLongValues")));
            pl::call(it, "Image::ExifTool::AddTagToTable", vec![tag_table_ptr.clone(), tag.clone(), tag_info.clone()])?;
            break;
        }

        // read value and uncompress if necessary
        let format_str = format_str_of(it, &format);
        if tag_info.truthy() || (format_str.eq_str("MIE") && band(&format, 0x04).truthy()) {
            if !raf_read_eq(it, &raf, &mut value, &val_len)? {
                break;
            }
            if band(&format, 0x04).truthy() {
                if verbose.truthy() {
                    print_out(it, &out, &[cat(&[&et.get("INDENT"), &s("[Tag '"), &tag, &s("' "), &val_len, &s(" bytes compressed]\n")])])?;
                }
                if !has_zlib_rs(it, &et, "decode")? {
                    continue;
                }
                let mut stat = Value::Undef;
                let inflate = pl::call(it, "Compress::Zlib::inflateInit", vec![])?;
                if inflate.truthy() {
                    let mut inp = value.clone();
                    let (o, st) = inflate_call(it, &inflate, &mut inp)?;
                    value = o;
                    stat = st;
                }
                if !(inflate.truthy() && pl::num_eq(&stat, &z_stream_end(it)?)) {
                    warn(it, &et, cat(&[&s("Error inflating "), &tag]))?;
                    continue;
                }
                val_len = len(&value);
                was_compressed = n(1);
            }
        }

        // process this tag
        if format_str.eq_str("MIE") {
            // process MIE directory
            let mut dir_name = Value::Undef;
            let sub_table_ptr = if tag_info.truthy() && tag_info.get("SubDirectory").truthy() {
                dir_name = tag_info.get("SubDirectory").get("DirName");
                let sub_table = tag_info.get("SubDirectory").get("TagTable");
                if sub_table.truthy() {
                    etf(it, "GetTagTable", vec![sub_table])?
                } else {
                    tag_table_ptr.clone()
                }
            } else {
                etf(it, "GetTagTable", vec![s("Image::ExifTool::MIE::Unknown")])?
            };
            if verbose.truthy() {
                let order = pl::concat(&s(", byte order "), &byte_order(it)?);
                pl::method(it, &et, "VerboseInfo", vec![tag.clone(), tag_info.clone(), s("Size"), val_len.clone(), s("Extra"), order])?;
            }
            let subdir_info = Value::new_hash();
            subdir_info.set("DirName", if dir_name.truthy() { dir_name } else { tag.clone() });
            subdir_info.set("RAF", raf.clone());
            subdir_info.set("Parent", dir_info.get("DirName"));
            subdir_info.set("WasCompressed", was_compressed.clone());
            // read from uncompressed data instead if necessary
            if val_len.truthy() {
                let r = pl::method(it, &s("File::RandomAccess"), "new", vec![pl::sref(value.clone())])?;
                subdir_info.set("RAF", r);
            }

            let old_order = byte_order(it)?;
            set_byte_order(it, if band(&format, 0x08).truthy() { s("II") } else { s("MM") })?;
            msg = process_mie_group_rs(it, &et, subdir_info, &sub_table_ptr)?;
            set_byte_order(it, old_order)?;
            et.set("SET_GROUP1", grp1.clone()); // restore this group1 name
            if msg.truthy() {
                break;
            }
        } else {
            // process MIE data format types
            if tag_info.truthy() {
                let rational = cell(Value::Undef);
                let mut bin_val = Value::Undef;
                // extract tag value
                let mut val = read_mie_value1(it, vec![pl::sref(value.clone()), n(0), format_str.clone(), Value::Undef, val_len.clone(), Value::ScalarRef(rational.clone())])?;
                if et.get("OPTIONS").get("SaveBin").truthy() {
                    bin_val = pl::substr(&value, 0, Some(val_len.to_i64()));
                }
                if val.is_undef() {
                    warn(it, &et, cat(&[&s("Error reading "), &tag, &s(" value")]))?;
                    val = s("<err>");
                }
                // save type or mime type
                if tag.eq_str("0Type") || tag.eq_str("2MIME") {
                    mime = val.clone();
                }
                if verbose.truthy() {
                    let mut count = Value::Undef;
                    let sz = pl::call(it, "Image::ExifTool::FormatSize", vec![format_str.clone()])?;
                    if sz.truthy() && !re(r"^(utf|string|undef)", "").is_match(&format_str) {
                        count = pl::div(&val_len, &sz)?;
                    }
                    let dp = if was_compressed.truthy() { Value::Undef } else { pl::sub(&tell(it, &raf)?, &val_len) };
                    pl::method(
                        it,
                        &et,
                        "VerboseInfo",
                        vec![last_tag.clone(), tag_info.clone(), s("DataPt"), pl::sref(value.clone()), s("DataPos"), dp, s("Size"), val_len.clone(), s("Format"), format_str.clone(), s("Value"), val.clone(), s("Count"), count],
                    )?;
                }
                if tag_info.get("SubDirectory").truthy() {
                    let sub_table_ptr = etf(it, "GetTagTable", vec![tag_info.get("SubDirectory").get("TagTable")])?;
                    let subdir_info = Value::new_hash();
                    subdir_info.set("DirName", tag_info.get("Name"));
                    subdir_info.set("DataPt", pl::sref(value.clone()));
                    subdir_info.set("DataLen", val_len.clone());
                    subdir_info.set("DirStart", n(0));
                    subdir_info.set("DirLen", val_len.clone());
                    subdir_info.set("Parent", dir_info.get("DirName"));
                    subdir_info.set("WasCompressed", was_compressed.clone());
                    // set DataPos and Base for uncompressed information only
                    if !was_compressed.truthy() {
                        subdir_info.set("DataPos", n(0)); // (relative to Base)
                        subdir_info.set("Base", pl::sub(&tell(it, &raf)?, &val_len));
                    }
                    // reset PROCESSED lookup for each MIE directory
                    // (there is no possibility of double-processing a MIE directory)
                    et.set("PROCESSED", Value::new_hash());
                    let process_proc = tag_info.get("SubDirectory").get("ProcessProc");
                    et.delete("SET_GROUP1");
                    et.delete("NO_LIST");
                    pl::method(it, &et, "ProcessDirectory", vec![subdir_info, sub_table_ptr, process_proc])?;
                    et.set("SET_GROUP1", grp1.clone());
                    et.set("NO_LIST", n(1));
                } else {
                    // convert to specified character set if necessary
                    if not_utf8 && re(r"^(utf|string)", "").is_match(&format_str) {
                        val = pl::method(it, &et, "Decode", vec![val.clone(), s("UTF8")])?;
                    }
                    if re(r"_list$", "").is_match(&format_str) {
                        // split list value into separate strings
                        let vals = pl::split(&re(r"\0", ""), &val, 0);
                        val = Value::new_array(vals);
                    }
                    if !units.is_undef() {
                        if val.is_ref() {
                            // convert string list to number list
                            let sep = it.get_scalar("main::\"");
                            let parts: Vec<Vec<u8>> = val.items().iter().map(|v| v.to_bytes().to_vec()).collect();
                            val = Value::bytes(parts.join(&sep.to_bytes()[..]));
                        }
                        // add units to value if specified
                        val = cat(&[&val, &s("("), &units, &s(")")]);
                    }
                    let key = pl::method(it, &et, "FoundTag", vec![tag_info.clone(), val])?;
                    if !key.is_undef() {
                        let ex = et.get_or_new_hash("TAG_EXTRA").get_or_new_hash(key.to_bytes());
                        let rat = get(&rational);
                        if !rat.is_undef() {
                            ex.set("Rational", rat);
                        }
                        if !bin_val.is_undef() {
                            ex.set("BinVal", bin_val);
                        }
                        if et.get("OPTIONS").get("SaveFormat").truthy() {
                            ex.set("G6", format_str.clone());
                        }
                    }
                }
            } else {
                // skip over unknown information or free bytes
                if !seek(it, &raf, val_len.clone(), 1)? {
                    msg = s("Seek error");
                    break;
                }
                if verbose.truthy() {
                    pl::method(it, &et, "VerboseInfo", vec![tag.clone(), Value::Undef, s("Size"), val_len.clone()])?;
                }
            }
        }
    }
    // modify MIME type if necessary
    if mime.truthy() && !dir_info.get("Parent").truthy() {
        pl::method(it, &et, "ModifyMimeType", vec![mime])?;
    }

    if !ok && !msg.truthy() {
        msg = s("Unexpected end of file");
    }
    if verbose.truthy() {
        et.set("INDENT", old_indent);
    }
    ret(msg)
}

/// Read/write a MIE file
/// Inputs: 0) ExifTool object reference, 1) DirInfo reference
/// Returns: 1 on success, 0 if this wasn't a valid MIE file, or -1 on write error
/// - process as a trailer if "Trailer" flag set in dirInfo
#[allow(unused_assignments)]
pub fn process_mie(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info) = (arg(&a, 0), arg(&a, 1));
    if a.is_empty() || et.is_undef() {
        return ret(n(1));
    }
    let raf = dir_info.get("RAF");
    let outfile = dir_info.get("OutFile");
    let (mut buff, mut err, mut msg, mut pos, mut end, mut is_creating) = (Value::Undef, false, Value::Undef, Value::Undef, Value::Undef, false);
    let mut num_docs = 0i64;
    //
    // process as a trailer (from end of file) if specified
    //
    if dir_info.get("Trailer").truthy() {
        let o = dir_info.get("Offset");
        let offset = if o.truthy() { o } else { n(0) }; // offset from end of file
        if !seek(it, &raf, pl::sub(&n(-10), &offset), 2)? {
            return ret(n(0));
        }
        loop {
            // read and validate last 10 bytes
            if !raf_read_eq(it, &raf, &mut buff, &n(10))? {
                break;
            }
            let Some(c) = re(r"~\0\0\x06.{4}(\x10|\x18)(\x04)$", "s").m(&buff).or_else(|| re(r"(\x10|\x18)(\x08)$", "s").m(&buff)) else { break };
            let (m1, m2) = (cap(&c, 1), cap(&c, 2));
            set_byte_order(it, if m1.eq_str("\x10") { s("MM") } else { s("II") })?;
            let l = if m2.eq_str("\x04") { g32u(it, &pl::sref(buff.clone()), n(4))? } else { etf(it, "Get64u", vec![pl::sref(buff.clone()), n(0)])? };
            let cur_pos = tell(it, &raf)?;
            if !cur_pos.truthy() {
                break;
            }
            if pl::lt(&l, &n(12)) || pl::gt(&l, &cur_pos) {
                break;
            }
            // validate element header if 8-byte offset was used
            if m2.eq_str("\x08") {
                if pl::lt(&l, &n(14)) {
                    break;
                }
                if !(seek(it, &raf, pl::sub(&cur_pos, &n(14)), 0)? && raf_read(it, &raf, &mut buff, n(4))?.truthy()) {
                    break;
                }
                if !buff.eq_str("~\0\0\x0a") {
                    break;
                }
            }
            // looks like a good group, so remember start position
            pos = pl::sub(&cur_pos, &l);
            if !end.truthy() {
                end = cur_pos.clone();
            }
            // seek to 10 bytes from end of previous group
            if !seek(it, &raf, pl::sub(&pos, &n(10)), 0)? {
                break;
            }
        }
        // seek to start of first MIE group
        if !(!pos.is_undef() && seek(it, &raf, pos.clone(), 0)?) {
            return ret(n(0));
        }
        // update DataPos and DirLen for ProcessTrailers()
        dir_info.set("DataPos", pos.clone());
        dir_info.set("DirLen", pl::sub(&end, &pos));
        if outfile.truthy() && et.get("DEL_GROUP").get("MIE").truthy() {
            // delete the trailer
            pl::method(it, &et, "VPrint", vec![n(0), s("  Deleting MIE trailer\n")])?;
            inc(&et, "CHANGED");
            return ret(n(1));
        } else if opt(it, &et, "Verbose")?.truthy() || et.get("HTML_DUMP").truthy() {
            pl::method(it, &et, "DumpTrailer", vec![dir_info.clone()])?;
        }
    }
    //
    // loop through all documents in MIE file
    //
    loop {
        // look for "0MIE" group element
        let num = raf_read(it, &raf, &mut buff, n(8))?;
        if pl::num_eq(&num, &n(8)) {
            // verify file identifier
            if let Some(c) = re(r"^~(\x10|\x18)\x04(.)0MIE", "s").m(&buff) {
                set_byte_order(it, if cap(&c, 1).eq_str("\x10") { s("MM") } else { s("II") })?;
                let l = crate::modules::write_exif::ord(&cap(&c, 2));
                // skip extended DataLength if it exists
                if pl::gt(&l, &n(252)) && !seek(it, &raf, ops::shift_left(&n(1), &pl::sub(&n(256), &l)), 1)? {
                    msg = s("Seek error");
                    break;
                }
            } else {
                if num_docs == 0 {
                    return ret(n(0)); // not a MIE file
                }
                if re(r"^~", "").is_match(&buff) {
                    msg = s("Non-standard file-level MIE element");
                } else {
                    msg = s("Invalid MIE file-level data");
                }
            }
        } else if num_docs != 0 {
            if !num.truthy() {
                break; // OK, all done with file
            }
            msg = s("Truncated MIE element header");
        } else {
            if num.truthy() || !outfile.truthy() {
                return ret(n(0));
            }
            // we have the ability to create a MIE file from scratch
            buff = s(""); // start from nothing
            // set byte order according to preferences
            pl::method(it, &et, "SetPreferredByteOrder", vec![])?;
            is_creating = true;
        }
        if msg.truthy() {
            if dir_info.get("Trailer").truthy() {
                break; // allow other trailers after MIE
            }
            if outfile.truthy() {
                pl::method(it, &et, "Error", vec![msg.clone()])?;
            } else {
                warn(it, &et, msg.clone())?;
            }
            break;
        }
        // this is a new MIE document -- increment document count
        if num_docs == 0 {
            // this is a valid MIE file (unless a trailer on another file)
            pl::method(it, &et, "SetFileType", vec![])?;
            et.set("NO_LIST", n(1)); // handle lists ourself
            et.set("MIE_COUNT", Value::new_hash());
            put(&has_zlib_var(it), Value::Undef);
        }
        num_docs += 1;

        // process the MIE groups recursively, beginning with the main MIE group
        let tag_table_ptr = etf(it, "GetTagTable", vec![s("Image::ExifTool::MIE::Main")])?;

        let subdir_info = Value::new_hash();
        subdir_info.set("DirName", s("MIE"));
        subdir_info.set("RAF", raf.clone());
        subdir_info.set("OutFile", outfile.clone());
        // don't define Parent so WriteMIEGroup() writes extended terminator
        if outfile.truthy() {
            // save start position in $outfile
            let op = out_pos(it, &outfile)?;
            subdir_info.set("OutPos", op);
            // generate lookup for MIE format codes if not done already
            let mie_code = mie_code(it);
            if mie_code.keys().is_empty() {
                let mie_format = mie_format(it);
                for k in keys(&mie_format) {
                    mie_code.set(mie_format.get(k.to_bytes()).to_bytes(), k);
                }
            }
            // update %mieMap with user-defined MIE groups
            if !get(&done_mie_map_var(it)).truthy() {
                update_mie_map(it, vec![], Want::Void)?;
            }
            // initialize write directories, with MIE tags taking priority
            // (note that this may re-initialize directories when writing trailer
            //  to another type of image, but this is OK because we are done writing
            //  the other format by the time we start writing the trailer)
            let mm = mie_map(it);
            pl::method(it, &et, "InitWriteDirs", vec![mm, s("MIE")])?;
            let gf = mie_group_format_rs(it, true)?;
            subdir_info.set("ToWrite", cat(&[&s("~"), &gf, &Value::bytes(b"\x04\xfe0MIE\0\0\0\0")]));
            msg = write_mie_group_rs(it, &et, subdir_info, &tag_table_ptr)?;
            if msg.truthy() {
                pl::method(it, &et, "Error", vec![msg.clone()])?;
                err = true;
                break;
            } else if !msg.is_undef() && is_creating {
                break;
            }
        } else {
            msg = process_mie_group_rs(it, &et, subdir_info, &tag_table_ptr)?;
            if msg.truthy() {
                warn(it, &et, msg.clone())?;
                break;
            }
        }
    }
    et.delete("NO_LIST");
    et.delete("MIE_COUNT");
    et.delete("SET_GROUP1");
    ret(n(if err { -1 } else { 1 }))
}

pub fn register(it: &mut Interp) {
    it.define_native("Image::ExifTool::MIE::UpdateMieMap", update_mie_map);
    it.define_native("Image::ExifTool::MIE::GetLangInfo", get_lang_info);
    it.define_native("Image::ExifTool::MIE::HasZlib", has_zlib);
    it.define_native("Image::ExifTool::MIE::MIEGroupFormat", mie_group_format);
    it.define_native("Image::ExifTool::MIE::ReadMIEValue", read_mie_value);
    it.define_native("Image::ExifTool::MIE::CheckMIE", check_mie);
    it.define_native("Image::ExifTool::MIE::WriteMIEGroup", write_mie_group);
    it.define_native("Image::ExifTool::MIE::ProcessMIEGroup", process_mie_group);
    it.define_native("Image::ExifTool::MIE::ProcessMIE", process_mie);
}
