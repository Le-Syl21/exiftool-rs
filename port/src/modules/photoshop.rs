//! Image::ExifTool::Photoshop (Image/ExifTool/Photoshop.pm), ported sub by sub (see docs/port/PORTING.md).

use crate::interp::eval::{die, Cell, ER};
use crate::interp::{Interp, Want};
use crate::modules::random_access::{cell, raf_read_cell, substr};
use crate::perl::{ops, Value};
use crate::pl::{self, arg, cap, re, ret};

fn n(i: i64) -> Value {
    Value::Int(i)
}

/// `\$var` of a `my` variable kept in a cell.
fn rf(c: &Cell) -> Value {
    Value::ScalarRef(c.clone())
}

fn val(c: &Cell) -> Value {
    c.borrow().clone()
}

fn put(c: &Cell, v: Value) {
    *c.borrow_mut() = v;
}

fn call(it: &mut Interp, name: &str, args: Vec<Value>) -> ER<Value> {
    pl::call(it, &format!("Image::ExifTool::{name}"), args)
}

/// `$raf->Read($buff, $len) == $len`
fn read_eq(it: &mut Interp, raf: &Value, buff: &Cell, len: &Value) -> ER<bool> {
    // (die here, as perl does, on an undefined or unblessed invocant: the
    //  read helper runs the call as interpreted code, with its own location)
    match raf {
        Value::Str(_) | Value::UStr(_) => {}
        Value::HashRef(h) if h.borrow().blessed.is_some() => {}
        _ => {
            pl::method(it, raf, "Read", vec![])?;
        }
    }
    let r = raf_read_cell(it, raf, buff, len.clone())?;
    Ok(pl::num_eq(&r, len))
}

fn seek(it: &mut Interp, raf: &Value, pos: Value, whence: i64) -> ER<bool> {
    Ok(pl::method(it, raf, "Seek", vec![pos, n(whence)])?.truthy())
}

fn tell(it: &mut Interp, raf: &Value) -> ER<Value> {
    pl::method(it, raf, "Tell", vec![])
}

/// The key/value pairs of a hash, in its order (`%dinfo` in a list).
fn flatten(h: &Value) -> Vec<Value> {
    h.keys().into_iter().flat_map(|k| [Value::from(k.clone()), h.get(&k)]).collect()
}

/// `pack 'N*', unpack 'V*', $dat`
fn swap_words(d: &Value) -> ER<Value> {
    let v = match crate::perl::pack::unpack(b"V*", &d.to_bytes()) {
        Ok(v) => v,
        Err(e) => return die(e),
    };
    match crate::perl::pack::pack(b"N*", &v) {
        Ok(b) => Ok(Value::bytes(b)),
        Err(e) => die(e),
    }
}

/// AutoLoad our writer routines when necessary
pub fn autoload(it: &mut Interp, a: Vec<Value>, want: Want) -> ER<Vec<Value>> {
    let name = it.get_scalar("Image::ExifTool::Photoshop::AUTOLOAD");
    let mut args = vec![name];
    args.extend(a);
    pl::at(718);
    it.call_named("Image::ExifTool::DoAutoLoad", args, want)
}

/// Convert pascal string(s) to something we can use
/// Inputs: 1) Pascal string data
/// Returns: Strings, concatenated with ', '
pub fn convert_pascal_string(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, in_str) = (arg(&a, 0), arg(&a, 1));
    let mut out_str = pl::s("");
    let len = pl::length(&in_str);
    let mut i = n(0);
    while pl::lt(&i, &len) {
        let c = substr(it, &in_str, &i, Some(&n(1)))?;
        let nn = n(c.to_bytes().first().copied().unwrap_or(0) as i64);
        if pl::ge(&pl::add(&i, &nn), &len) {
            break;
        }
        if i.truthy() {
            out_str = pl::concat(&out_str, &pl::s(", "));
        }
        let part = substr(it, &in_str, &pl::add(&i, &n(1)), Some(&nn))?;
        out_str = pl::concat(&out_str, &part);
        i = pl::add(&i, &pl::add(&nn, &n(1)));
    }
    let cs = pl::method(it, &et, "Options", vec![pl::s("CharsetPhotoshop")])?;
    let charset = if cs.truthy() { cs } else { pl::s("Latin") };
    ret(pl::method(it, &et, "Decode", vec![out_str, charset])?)
}

/// Run `f` with `$_` localized (`local $_;`).
fn local_topic<T>(it: &mut Interp, f: impl FnOnce(&mut Interp) -> ER<T>) -> ER<T> {
    let saved = it.get_scalar("main::_");
    it.set_scalar("main::_", Value::Undef);
    let r = f(it);
    it.set_scalar("main::_", saved);
    r
}

/// Process Photoshop layers and mask information section of PSD/PSB file
/// Inputs: 0) ExifTool ref, 1) DirInfo ref, 2) tag table ref
/// Returns: 1 on success (and seeks to the end of this section)
pub fn process_layers_and_mask(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    local_topic(it, |it| {
        let (et, dir_info, tag_table_ptr) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
        let raf = dir_info.get("RAF");
        let file_type = et.get("FileType");
        let data = cell(Value::Undef);

        if !(file_type.eq_str("PSD") || file_type.eq_str("PSB")) {
            return ret(n(0)); // (no layer section in CS1 files)
        }

        // (some words are 4 bytes in PSD files and 8 bytes in PSB)
        let (psb, psiz) = if file_type.eq_str("PSB") { (n(1), n(8)) } else { (Value::Undef, n(4)) };
        let get_len = |it: &mut Interp, data: &Cell| -> ER<Value> { if psb.truthy() { call(it, "Get64u", vec![rf(data), n(0)]) } else { call(it, "Get32u", vec![rf(data), n(0)]) } };

        // read the layer information header
        let nn = pl::add(&pl::mul(&psiz, &n(2)), &n(2));
        pl::at(761);
        if !read_eq(it, &raf, &data, &nn)? {
            return ret(n(0));
        }
        let tot = get_len(it, &data)?; // length of layer and mask info
        if pl::num_eq(&tot, &n(0)) {
            return ret(n(1));
        }
        let end = pl::add(&pl::sub(&pl::sub(&tell(it, &raf)?, &psiz), &n(2)), &tot);
        let d = substr(it, &val(&data), &psiz, None)?;
        put(&data, d);
        let mut len = get_len(it, &data)?; // length of layer info section
        let num = call(it, "Get16s", vec![rf(&data), psiz.clone()])?;
        // check for Lr16 block if layers length is 0 (ref https://forums.adobe.com/thread/1540914)
        if pl::num_eq(&len, &n(0)) && pl::num_eq(&num, &n(0)) {
            if !read_eq(it, &raf, &data, &n(10))? {
                return ret(n(0));
            }
            if re(r"^..8BIMLr16", "s").is_match(&val(&data)) {
                if !read_eq(it, &raf, &data, &pl::add(&psiz, &n(2)))? {
                    return ret(n(0));
                }
                len = get_len(it, &data)?;
            } else if re(r"^..8BIMMt16", "s").is_match(&val(&data)) {
                // (have seen Mt16 before Lr16, ref PH)
                if !read_eq(it, &raf, &data, &psiz)? {
                    return ret(n(0));
                }
                if !read_eq(it, &raf, &data, &n(8))? {
                    return ret(n(0));
                }
                if val(&data).eq_str("8BIMLr16") {
                    if !read_eq(it, &raf, &data, &pl::add(&psiz, &n(2)))? {
                        return ret(n(0));
                    }
                    len = get_len(it, &data)?;
                } else if !seek(it, &raf, pl::sub(&n(-18), &psiz), 1)? {
                    return ret(n(0));
                }
            } else if !seek(it, &raf, n(-10), 1)? {
                return ret(n(0));
            }
        }
        len = pl::add(&len, &n(2)); // include layer count with layer info section
        if !seek(it, &raf, n(-2), 1)? {
            return ret(n(0));
        }
        let dinfo = Value::new_hash();
        dinfo.set("RAF", raf.clone());
        dinfo.set("DirLen", len);
        et.set("IsPSB", psb.clone()); // set PSB flag
        process_layers(it, vec![et.clone(), dinfo, tag_table_ptr.clone()], Want::Void)?;

        // seek to the end of this section and return success flag
        ret(if seek(it, &raf, end, 0)? { n(1) } else { n(0) })
    })
}

/// Process Photoshop channel options (ref forum16762)
/// Inputs: 0) ExifTool ref, 1) DirInfo ref, 2) tag table ref
/// Returns: 1 on success
pub fn process_channel_options(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info, tag_table_ptr) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    let end = pl::add(&dir_info.get("DirStart"), &dir_info.get("DirLen"));
    dir_info.set("DirLen", n(13));
    let mut i = n(0);
    while pl::le(&pl::add(&dir_info.get("DirStart"), &n(13)), &end) {
        et.set("SET_GROUP1", pl::concat(&pl::s("Channel"), &i));
        pl::method(it, &et, "ProcessBinaryData", vec![dir_info.clone(), tag_table_ptr.clone()])?;
        dir_info.set("DirStart", pl::add(&dir_info.get("DirStart"), &n(13)));
        i = pl::add(&i, &n(1));
    }
    et.delete("SET_GROUP1");
    ret(n(1))
}

/// Process Photoshop layers (beginning with layer count)
/// Inputs: 0) ExifTool ref, 1) DirInfo ref, 2) tag table ref
/// Returns: 1 on success
/// Notes: Uses ExifTool IsPSB member to determine whether file is PSB format
pub fn process_layers(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    local_topic(it, |it| process_layers_body(it, a))
}

fn process_layers_body(it: &mut Interp, a: Vec<Value>) -> ER<Vec<Value>> {
    let (et, dir_info, tag_table_ptr) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    let (buff, buf2) = (cell(Value::Undef), cell(Value::Undef));
    let mut nn;
    // %count: the tags seen, in order of appearance
    let count = Value::new_hash();
    let raf = dir_info.get("RAF");
    let dir_len = dir_info.get("DirLen");
    let verbose = et.path(&["OPTIONS", "Verbose"]);
    let dinfo = Value::new_hash();
    dinfo.set("DataPt", rf(&buff));
    pl::at(832);
    let base = tell(it, &raf)?;
    dinfo.set("Base", base);
    let mut pos = n(0);
    if pl::lt(&dir_len, &n(2)) {
        return ret(n(0));
    }
    if !read_eq(it, &raf, &buff, &n(2))? {
        return ret(n(0));
    }
    let mut num = call(it, "Get16s", vec![rf(&buff), n(0)])?; // number of layers
    if pl::lt(&num, &n(0)) {
        num = pl::neg(&num); // (first channel is transparency data if negative)
    }
    pl::method(it, &et, "VerboseDir", vec![pl::s("Layers"), num.clone(), dir_len.clone()])?;
    let handle = |it: &mut Interp, tag: &str, v: Value, pre: Vec<Value>, dinfo: &Value| -> ER<Value> {
        let mut args = vec![tag_table_ptr.clone(), pl::s(tag), v];
        args.extend(pre);
        args.extend(flatten(dinfo));
        pl::method(it, &et, "HandleTag", args)
    };
    handle(it, "_xcnt", num.clone(), vec![pl::s("Start"), pos.clone(), pl::s("Size"), n(2)], &dinfo)?; // LayerCount
    let old_indent = et.get("INDENT");
    et.set("INDENT", pl::concat(&et.get("INDENT"), &pl::s("| ")));
    pos = pl::add(&pos, &n(2));
    let psb = et.get("IsPSB"); // is PSB format?
    let psiz = if psb.truthy() { n(8) } else { n(4) };
    let mut i = n(0);
    'layer: while pl::lt(&i, &num) {
        // process each layer
        let msg = pl::concat(&pl::concat(&pl::concat(&pl::concat(&old_indent, &pl::s("+ [Layer ")), &pl::add(&i, &n(1))), &pl::concat(&pl::s(" of "), &num)), &pl::s("]\n"));
        pl::method(it, &et, "VPrint", vec![n(0), msg])?;
        if pl::gt(&pl::add(&pos, &n(18)), &dir_len) {
            break;
        }
        if !read_eq(it, &raf, &buff, &n(18))? {
            break;
        }
        dinfo.set("DataPos", pos.clone());
        // save the layer rectangle
        handle(it, "_xrct", Value::Undef, vec![pl::s("Start"), n(0), pl::s("Size"), n(16)], &dinfo)?;
        let num_channels = call(it, "Get16u", vec![rf(&buff), n(16)])?;
        nn = pl::mul(&pl::add(&n(2), &psiz), &num_channels); // size of channel information
        if !seek(it, &raf, nn.clone(), 1)? {
            break;
        }
        pos = pl::add(&pos, &pl::add(&n(18), &nn));
        if pl::gt(&pl::add(&pos, &n(20)), &dir_len) {
            break;
        }
        if !read_eq(it, &raf, &buff, &n(20))? {
            break;
        }
        dinfo.set("DataPos", pos.clone());
        let sig = substr(it, &val(&buff), &n(0), Some(&n(4)))?;
        if !re(r"^(8BIM|MIB8)$", "").is_match(&sig) {
            break; // verify signature
        }
        handle(it, "_xbnd", Value::Undef, vec![pl::s("Start"), n(4), pl::s("Size"), n(4)], &dinfo)?;
        handle(it, "_xopc", Value::Undef, vec![pl::s("Start"), n(8), pl::s("Size"), n(1)], &dinfo)?;
        handle(it, "_xvis", Value::Undef, vec![pl::s("Start"), n(10), pl::s("Size"), n(1)], &dinfo)?;
        let nxt = pl::add(&pl::add(&pos, &n(16)), &call(it, "Get32u", vec![rf(&buff), n(12)])?);
        nn = call(it, "Get32u", vec![rf(&buff), n(16)])?; // get size of layer mask data
        pos = pl::add(&pos, &pl::add(&n(20), &nn)); // skip layer mask data
        if pl::gt(&pl::add(&pos, &n(4)), &dir_len) {
            break;
        }
        if !(seek(it, &raf, nn.clone(), 1)? && read_eq(it, &raf, &buff, &n(4))?) {
            break;
        }
        nn = call(it, "Get32u", vec![rf(&buff), n(0)])?; // get size of layer blending ranges
        pos = pl::add(&pos, &pl::add(&n(4), &nn)); // skip layer blending ranges data
        if pl::gt(&pl::add(&pos, &n(1)), &dir_len) {
            break;
        }
        if !(seek(it, &raf, nn.clone(), 1)? && read_eq(it, &raf, &buff, &n(1))?) {
            break;
        }
        nn = call(it, "Get8u", vec![rf(&buff), n(0)])?; // get length of layer name
        if pl::gt(&pl::add(&pl::add(&pos, &n(1)), &nn), &dir_len) {
            break;
        }
        if !read_eq(it, &raf, &buff, &nn)? {
            break;
        }
        dinfo.set("DataPos", pl::add(&pos, &n(1)));
        handle(it, "_xnam", Value::Undef, vec![pl::s("Start"), n(0), pl::s("Size"), nn.clone()], &dinfo)?;
        let mut frag = ops::bit_and(&pl::add(&nn, &n(1)), &n(0x3));
        if frag.truthy() && !seek(it, &raf, pl::sub(&n(4), &frag), 1)? {
            break;
        }
        nn = ops::bit_and(&pl::add(&nn, &n(4)), &n(0xfffffffc)); // +1 for length byte then pad to multiple of 4 bytes
        pos = pl::add(&pos, &nn);
        // process additional layer info
        while pl::le(&pl::add(&pos, &n(12)), &nxt) {
            if !read_eq(it, &raf, &buff, &n(12))? {
                break;
            }
            let mut dat = substr(it, &val(&buff), &n(0), Some(&n(8)))?;
            if call(it, "GetByteOrder", vec![])?.eq_str("II") {
                dat = swap_words(&dat)?;
            }
            let sig = substr(it, &dat, &n(0), Some(&n(4)))?;
            if !(sig.eq_str("8BIM") || sig.eq_str("8B64")) {
                break; // verify signature
            }
            let tag = substr(it, &dat, &n(4), Some(&n(4)))?;
            // (some structures have an 8-byte size word [augh!]
            // --> it would be great if '8B64' indicated a 64-bit version, and this may well
            // be the case, but it is not mentioned in the Photoshop file format specification)
            if psb.truthy() && re(r"^(LMsk|Lr16|Lr32|Layr|Mt16|Mt32|Mtrn|Alph|FMsk|lnk2|FEid|FXid|PxSD)$", "").is_match(&tag) {
                if pl::gt(&pl::add(&pos, &n(16)), &nxt) {
                    break;
                }
                if !read_eq(it, &raf, &buf2, &n(4))? {
                    break;
                }
                let b = pl::concat(&val(&buff), &val(&buf2));
                put(&buff, b);
                nn = call(it, "Get64u", vec![rf(&buff), n(8)])?;
                pos = pl::add(&pos, &n(4));
            } else {
                nn = call(it, "Get32u", vec![rf(&buff), n(8)])?;
            }
            pos = pl::add(&pos, &n(12));
            if pl::gt(&pl::add(&pos, &nn), &nxt) {
                break;
            }
            frag = ops::bit_and(&nn, &n(0x3));
            if tag_table_ptr.get(tag.to_bytes()).truthy() || verbose.truthy() {
                // pad with empty entries if necessary to keep the same index for each item in the layer
                if count.get(tag.to_bytes()).is_undef() {
                    count.set(tag.to_bytes(), n(0));
                }
                if !read_eq(it, &raf, &buff, &nn)? {
                    break;
                }
                dinfo.set("DataPos", pos.clone());
                while pl::lt(&count.get(tag.to_bytes()), &i) {
                    let v = if tag.eq_str("lsct") { n(0) } else { pl::s("") };
                    pl::method(it, &et, "HandleTag", vec![tag_table_ptr.clone(), tag.clone(), v])?;
                    count.set(tag.to_bytes(), pl::add(&count.get(tag.to_bytes()), &n(1)));
                }
                let mut args = vec![tag_table_ptr.clone(), tag.clone(), Value::Undef, pl::s("Start"), n(0), pl::s("Size"), nn.clone()];
                args.extend(flatten(&dinfo));
                pl::method(it, &et, "HandleTag", args)?;
                count.set(tag.to_bytes(), pl::add(&count.get(tag.to_bytes()), &n(1)));
                if frag.truthy() {
                    if !seek(it, &raf, pl::sub(&n(4), &frag), 1)? {
                        break;
                    }
                    nn = pl::add(&nn, &pl::sub(&n(4), &frag)); // pad to multiple of 4 bytes (PH NC)
                }
            } else {
                if frag.truthy() {
                    nn = pl::add(&nn, &pl::sub(&n(4), &frag));
                }
                if !seek(it, &raf, nn.clone(), 1)? {
                    break;
                }
            }
            pos = pl::add(&pos, &nn); // step to start of next structure
        }
        pos = nxt;
        i = pl::add(&i, &n(1));
        continue 'layer;
    }
    // pad lists if necessary to have an entry for each layer
    let mut keys = count.keys();
    keys.sort();
    for k in keys {
        let kv = Value::from(k.clone());
        it.set_scalar("main::_", kv.clone());
        while pl::lt(&count.get(&k), &num) {
            let v = if kv.eq_str("lsct") { n(0) } else { pl::s("") };
            pl::method(it, &et, "HandleTag", vec![tag_table_ptr.clone(), kv.clone(), v])?;
            count.set(&k, pl::add(&count.get(&k), &n(1)));
        }
    }
    et.set("INDENT", old_indent);
    ret(n(1))
}

/// Process Photoshop ImageSourceData
/// Inputs: 0) ExifTool ref, 1) dirInfo ref, 2) tag table ref
/// Returns: 1 on success
pub fn process_document_data(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info, tag_table_ptr) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    let verbose = et.path(&["OPTIONS", "Verbose"]);
    let mut raf = dir_info.get("RAF");
    let mut dir_len = dir_info.get("DirLen");
    let mut pos = n(36); // length of header
    let buff = cell(Value::Undef);
    let mut nn;
    let mut err = Value::Undef;

    pl::method(it, &et, "VerboseDir", vec![pl::s("Photoshop Document Data"), Value::Undef, dir_len.clone()])?;
    if !raf.truthy() {
        let data_pt = dir_info.get("DataPt");
        let ds = dir_info.get("DirStart");
        let start = if ds.truthy() { ds } else { n(0) };
        raf = pl::method(it, &pl::s("File::RandomAccess"), "new", vec![data_pt.clone()])?;
        if start.truthy() {
            seek(it, &raf, start.clone(), 0)?;
        }
        if dir_len.is_undef() {
            // (length $$dataPt - $start: perl parses it as length($$dataPt - $start))
            pl::at(958);
            dir_len = pl::length(&pl::sub(&pl::sderef(&data_pt)?, &start));
        }
        pl::method(it, &et, "VerboseDump", vec![data_pt, pl::s("Start"), start, pl::s("Len"), dir_len.clone(), pl::s("Base"), dir_info.get("Base")])?;
    }
    let m = if read_eq(it, &raf, &buff, &pos)? { re(r"^Adobe Photoshop Document Data (Block|V0002)\0", "").m(&val(&buff)) } else { None };
    let Some(m) = m else {
        pl::method(it, &et, "Warn", vec![pl::s("Invalid Photoshop Document Data")])?;
        return ret(n(0));
    };
    let psb = cap(&m, 1).eq_str("V0002");
    let dinfo = Value::new_hash();
    dinfo.set("DataPt", rf(&buff));
    et.set("IsPSB", Value::from(psb)); // set PSB flag (needed when handling Layers directory)
    while pl::le(&pl::add(&pos, &n(12)), &dir_len) {
        if !read_eq(it, &raf, &buff, &n(8))? {
            err = pl::s("Error reading document data");
            break;
        }
        // set byte order according to byte order of first signature
        if pl::num_eq(&pos, &n(36)) {
            let o = if re(r"^(8BIM|8B64)", "").is_match(&val(&buff)) { "MM" } else { "II" };
            call(it, "SetByteOrder", vec![pl::s(o)])?;
        }
        if call(it, "GetByteOrder", vec![])?.eq_str("II") {
            let b = swap_words(&val(&buff))?;
            put(&buff, b);
        }
        let sig = substr(it, &val(&buff), &n(0), Some(&n(4)))?;
        if !(sig.eq_str("8BIM") || sig.eq_str("8B64")) {
            err = pl::s("Bad photoshop resource"); // verify signature
            break;
        }
        let tag = substr(it, &val(&buff), &n(4), Some(&n(4)))?;
        if psb && re(r"^(LMsk|Lr16|Lr32|Layr|Mt16|Mt32|Mtrn|Alph|FMsk|lnk2|FEid|FXid|PxSD)$", "").is_match(&tag) {
            if pl::gt(&pl::add(&pos, &n(16)), &dir_len) {
                err = pl::s("Short PSB resource");
                break;
            }
            if !read_eq(it, &raf, &buff, &n(8))? {
                err = pl::s("Error reading PSB resource");
                break;
            }
            nn = call(it, "Get64u", vec![rf(&buff), n(0)])?;
            pos = pl::add(&pos, &n(4));
        } else {
            if !read_eq(it, &raf, &buff, &n(4))? {
                err = pl::s("Error reading PSD resource");
                break;
            }
            nn = call(it, "Get32u", vec![rf(&buff), n(0)])?;
        }
        pos = pl::add(&pos, &n(12));
        if pl::gt(&pl::add(&pos, &nn), &dir_len) {
            err = pl::s("Truncated photoshop resource");
            break;
        }
        let pad = ops::bit_and(&pl::sub(&n(4), &ops::bit_and(&nn, &n(3))), &n(3)); // number of padding bytes
        let tag_info = tag_table_ptr.get(tag.to_bytes());
        if tag_info.truthy() || verbose.truthy() {
            if tag_info.truthy() && tag_info.get("SubDirectory").truthy() {
                let fpos = pl::add(&pl::add(&tell(it, &raf)?, &nn), &pad);
                let sub_table = call(it, "GetTagTable", vec![tag_info.path(&["SubDirectory", "TagTable"])])?;
                let di = Value::new_hash();
                di.set("RAF", raf.clone());
                di.set("DirLen", nn.clone());
                pl::method(it, &et, "ProcessDirectory", vec![di, sub_table])?;
                if !seek(it, &raf, fpos, 0)? {
                    err = pl::s("Seek error");
                    break;
                }
            } else {
                let t = tell(it, &raf)?;
                dinfo.set("DataPos", t);
                dinfo.set("Start", n(0));
                dinfo.set("Size", nn.clone());
                if !read_eq(it, &raf, &buff, &nn)? {
                    err = pl::s("Error reading photoshop resource");
                    break;
                }
                let mut args = vec![tag_table_ptr.clone(), tag.clone(), Value::Undef];
                args.extend(flatten(&dinfo));
                pl::method(it, &et, "HandleTag", args)?;
                if !seek(it, &raf, pad.clone(), 1)? {
                    err = pl::s("Seek error");
                    break;
                }
            }
        } else if !seek(it, &raf, pl::add(&nn, &pad), 1)? {
            err = pl::s("Seek error");
            break;
        }
        pos = pl::add(&pos, &pl::add(&nn, &pad)); // step to start of next structure
    }
    if err.truthy() {
        pl::method(it, &et, "Warn", vec![err])?;
    }
    ret(n(1))
}

/// Process Photoshop APP13 record
/// Inputs: 0) ExifTool object reference, 1) Reference to directory information
///         2) Tag table reference
/// Returns: 1 on success
pub fn process_photoshop(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info, tag_table_ptr) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    let data_pt = dir_info.get("DataPt");
    let mut pos = dir_info.get("DirStart");
    let dir_end = pl::add(&pos, &dir_info.get("DirLen"));
    let verbose = pl::method(it, &et, "Options", vec![pl::s("Verbose")])?;
    let mut success = n(0);

    // ignore non-standard XMP while in strict MWG compatibility mode
    let strict = it.get_scalar("Image::ExifTool::MWG::strict");
    if (strict.truthy() || pl::method(it, &et, "Options", vec![pl::s("Validate")])?.truthy()) && re(r"^(JPEG|TIFF|PSD)$", "").is_match(&et.get("FILE_TYPE")) {
        let path = pl::method(it, &et, "MetadataPath", vec![])?;
        if !re(r"^(JPEG-APP13-Photoshop|TIFF-IFD0-Photoshop|PSD)$", "").is_match(&path) {
            if it.get_scalar("Image::ExifTool::MWG::strict").truthy() {
                pl::method(it, &et, "Warn", vec![pl::concat(&pl::s("Ignored non-standard Photoshop at "), &path)])?;
                return ret(n(1));
            } else {
                pl::method(it, &et, "Warn", vec![pl::concat(&pl::s("Non-standard Photoshop at "), &path), n(1)])?;
            }
        }
    }
    if et.get("FILE_TYPE").eq_str("JPEG") && !pl::str_eq(&dir_info.get("Parent"), "APP13") {
        et.get_or_new_hash("LOW_PRIORITY_DIR").set("*", n(1)); // lower priority of all these tags
    }
    call(it, "SetByteOrder", vec![pl::s("MM")])?; // Photoshop is always big-endian
    if verbose.truthy() {
        pl::method(it, &et, "VerboseDir", vec![pl::s("Photoshop"), n(0), dir_info.get("DirLen")])?;
    }

    // scan through resource blocks:
    // Format: 0) Type, 4 bytes - '8BIM' (or the rare 'PHUT', 'DCSR', 'AgHg' or 'MeSa')
    //         1) TagID,2 bytes
    //         2) Name, pascal string padded to even no. bytes
    //         3) Size, 4 bytes - N
    //         4) Data, N bytes
    while pl::lt(&pl::add(&pos, &n(8)), &dir_end) {
        pl::at(1055);
        let mut type_ = substr(it, &pl::sderef(&data_pt)?, &pos, Some(&n(4)))?;
        let tt_ptr;
        let (mut extra, mut val, name);
        extra = Value::Undef;
        val = Value::Undef;
        if type_.eq_str("8BIM") {
            tt_ptr = tag_table_ptr.clone();
        } else if re(r"^(PHUT|DCSR|AgHg|MeSa)$", "").is_match(&type_) {
            // (PHUT~ImageReady, MeSa~PhotoDeluxe)
            tt_ptr = call(it, "GetTagTable", vec![pl::s("Image::ExifTool::Photoshop::Unknown")])?;
        } else {
            re(r"([^\w])", "").subst(&mut type_, true, |c| {
                let o = cap(c, 1).to_bytes().first().copied().unwrap_or(0) as i64;
                pl::sprintf("\\x%.2x", &[n(o)]).to_bytes().to_vec()
            });
            let msg = pl::concat(&pl::concat(&pl::s("Bad Photoshop IRB resource \""), &type_), &pl::s("\""));
            pl::method(it, &et, "Warn", vec![msg])?;
            break;
        }
        let tag = call(it, "Get16u", vec![data_pt.clone(), pl::add(&pos, &n(4))])?;
        pos = pl::add(&pos, &n(6)); // point to start of name
        let name_len = call(it, "Get8u", vec![data_pt.clone(), pos.clone()])?;
        pos = pl::add(&pos, &n(1));
        let name_pos = pos.clone();
        // skip resource block name (pascal string, padded to an even # of bytes)
        pos = pl::add(&pos, &name_len);
        if !ops::bit_and(&name_len, &n(0x01)).truthy() {
            pos = pl::add(&pos, &n(1));
        }
        if pl::gt(&pl::add(&pos, &n(4)), &dir_end) {
            pl::method(it, &et, "Warn", vec![pl::s("Bad Photoshop resource block")])?;
            break;
        }
        let mut size = call(it, "Get32u", vec![data_pt.clone(), pos.clone()])?;
        pos = pl::add(&pos, &n(4));
        if pl::gt(&pl::add(&size, &pos), &dir_end) {
            pl::method(it, &et, "Warn", vec![pl::concat(&pl::s("Bad Photoshop resource data size "), &size)])?;
            break;
        }
        success = n(1);
        if name_len.truthy() {
            name = substr(it, &data_pt.deref(), &name_pos, Some(&name_len))?;
            extra = pl::concat(&pl::concat(&pl::s(", Name=\""), &name), &pl::s("\""));
        } else {
            name = pl::s("");
        }
        let tag_info = pl::method(it, &et, "GetTagInfo", vec![tt_ptr.clone(), tag.clone()])?;
        // append resource name to value if requested (braced by "/#...#/")
        if tag_info.truthy() && !tag_info.get("SetResourceName").is_undef() && tag_info.get("SetResourceName").eq_str("1") && !re(r"/#", "").is_match(&name) {
            let v = substr(it, &data_pt.deref(), &pos, Some(&size))?;
            val = pl::concat(&pl::concat(&pl::concat(&v, &pl::s("/#")), &name), &pl::s("#/"));
        }
        pl::method(
            it,
            &et,
            "HandleTag",
            vec![
                tt_ptr,
                tag,
                val,
                pl::s("TagInfo"),
                tag_info,
                pl::s("Extra"),
                extra,
                pl::s("DataPt"),
                data_pt.clone(),
                pl::s("DataPos"),
                dir_info.get("DataPos"),
                pl::s("Size"),
                size.clone(),
                pl::s("Start"),
                pos.clone(),
                pl::s("Base"),
                dir_info.get("Base"),
                pl::s("Parent"),
                dir_info.get("DirName"),
            ],
        )?;
        if ops::bit_and(&size, &n(0x01)).truthy() {
            size = pl::add(&size, &n(1)); // size is padded to an even # bytes
        }
        pos = pl::add(&pos, &size);
    }
    // warn about incorrect IPTCDigest
    let value = et.get("VALUE");
    if value.get("IPTCDigest").truthy() && value.get("CurrentIPTCDigest").truthy() && value.get("IPTCDigest").to_bytes() != value.get("CurrentIPTCDigest").to_bytes() {
        pl::method(it, &et, "Warn", vec![pl::s("IPTCDigest is not current. XMP may be out of sync")])?;
    }
    et.get_or_new_hash("LOW_PRIORITY_DIR").delete("*");
    ret(success)
}

/// extract information from Photoshop PSD file
/// Inputs: 0) ExifTool object reference, 1) dirInfo reference
/// Returns: 1 if this was a valid PSD file, -1 on write error
pub fn process_psd(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info) = (arg(&a, 0), arg(&a, 1));
    let raf = dir_info.get("RAF");
    let outfile = dir_info.get("OutFile");
    let data = cell(Value::Undef);
    let mut err = Value::Undef;
    let mut tag_table_ptr;

    pl::at(1131);
    if !read_eq(it, &raf, &data, &n(30))? {
        return ret(n(0));
    }

    let Some(m) = re(r"^8BPS\0([\x01\x02])", "").m(&val(&data)) else { return ret(n(0)) };
    call(it, "SetByteOrder", vec![pl::s("MM")])?;
    let ft = if cap(&m, 1).eq_str("\x01") { "PSD" } else { "PSB" };
    pl::method(it, &et, "SetFileType", vec![pl::s(ft)])?; // set the FileType tag
    let dir_info2 = Value::new_hash();
    dir_info2.set("DataPt", rf(&data));
    dir_info2.set("DirStart", n(0));
    dir_info2.set("DirName", pl::s("Photoshop"));
    let mut len = call(it, "Get32u", vec![rf(&data), n(26)])?;
    let write = |it: &mut Interp, args: Vec<Value>| -> ER<bool> { Ok(call(it, "Write", args)?.truthy()) };
    if outfile.truthy() {
        if !write(it, vec![outfile.clone(), val(&data)])? {
            err = n(1);
        }
        if !read_eq(it, &raf, &data, &len)? {
            return ret(n(-1));
        }
        if !write(it, vec![outfile.clone(), val(&data)])? {
            err = n(1); // write color mode data
        }
        // initialize map of where things are written
        let psd_map = crate::runtime::lex_hash(it, "Image::ExifTool::Photoshop", "psdMap");
        pl::method(it, &et, "InitWriteDirs", vec![psd_map])?;
    } else {
        // process the header
        tag_table_ptr = call(it, "GetTagTable", vec![pl::s("Image::ExifTool::Photoshop::Header")])?;
        dir_info2.set("DirLen", n(30));
        pl::method(it, &et, "ProcessDirectory", vec![dir_info2.clone(), tag_table_ptr.clone()])?;
        if !seek(it, &raf, len.clone(), 1)? {
            err = n(1); // skip over color mode data
        }
    }
    // read image resource section
    if !read_eq(it, &raf, &data, &n(4))? {
        err = n(1);
    }
    len = call(it, "Get32u", vec![rf(&data), n(0)])?;
    if !read_eq(it, &raf, &data, &len)? {
        err = n(1);
    }
    tag_table_ptr = call(it, "GetTagTable", vec![pl::s("Image::ExifTool::Photoshop::Main")])?;
    dir_info2.set("DirLen", len.clone());
    let mut rtn_val = n(1);
    if outfile.truthy() {
        // rewrite IRB resources
        put(&data, pl::call(it, "Image::ExifTool::Photoshop::WritePhotoshop", vec![et.clone(), dir_info2.clone(), tag_table_ptr.clone()])?);
        if val(&data).truthy() {
            len = call(it, "Set32u", vec![pl::length(&val(&data))])?;
            if !write(it, vec![outfile.clone(), len.clone(), val(&data)])? {
                err = n(1);
            }
            // look for trailer and edit if necessary
            let mut trail_info = pl::method(it, &et, "IdentifyTrailer", vec![raf.clone()])?;
            if trail_info.truthy() {
                let tbuf = Value::binary(b"");
                trail_info.set("OutFile", tbuf); // rewrite trailer(s)
                // rewrite all trailers to buffer
                if pl::method(it, &et, "ProcessTrailers", vec![trail_info.clone()])?.truthy() {
                    let mut copy_bytes = pl::sub(&trail_info.get("DataPos"), &tell(it, &raf)?);
                    if pl::ge(&copy_bytes, &n(0)) {
                        // copy remaining PSD file up to start of trailer
                        while copy_bytes.truthy() {
                            let nn = if pl::gt(&copy_bytes, &n(65536)) { n(65536) } else { copy_bytes.clone() };
                            if !read_eq(it, &raf, &data, &nn)? {
                                err = n(1);
                            }
                            if !write(it, vec![outfile.clone(), val(&data)])? {
                                err = n(1);
                            }
                            copy_bytes = pl::sub(&copy_bytes, &nn);
                        }
                        // write the trailer (or not)
                        if !pl::method(it, &et, "WriteTrailerBuffer", vec![trail_info.clone(), outfile.clone()])?.truthy() {
                            err = n(1);
                        }
                    } else {
                        pl::method(it, &et, "Warn", vec![pl::s("Overlapping trailer")])?;
                        trail_info = Value::Undef;
                    }
                } else {
                    trail_info = Value::Undef;
                }
            }
            if !trail_info.truthy() {
                // copy over the rest of the file
                while raf_read_cell(it, &raf, &data, n(65536))?.truthy() {
                    if !write(it, vec![outfile.clone(), val(&data)])? {
                        err = n(1);
                    }
                }
            }
        } else {
            err = n(1);
        }
        if err.truthy() {
            rtn_val = n(-1);
        }
    } else if err.truthy() {
        pl::method(it, &et, "Warn", vec![pl::s("File format error")])?;
    } else {
        // read IRB resources
        process_photoshop(it, vec![et.clone(), dir_info2.clone(), tag_table_ptr.clone()], Want::Void)?;
        // read layer and mask information section
        dir_info2.set("RAF", raf.clone());
        tag_table_ptr = call(it, "GetTagTable", vec![pl::s("Image::ExifTool::Photoshop::Layers")])?;
        let old_indent = et.get("INDENT");
        et.set("INDENT", pl::concat(&et.get("INDENT"), &pl::s("| ")));
        if process_layers_and_mask(it, vec![et.clone(), dir_info2.clone(), tag_table_ptr.clone()], Want::Scalar)?.pop().unwrap_or_default().truthy()
            // read compression mode from image data section
            && read_eq(it, &raf, &data, &n(2))?
        {
            let di = Value::new_hash();
            di.set("DataPt", rf(&data));
            di.set("DataPos", pl::sub(&tell(it, &raf)?, &n(2)));
            tag_table_ptr = call(it, "GetTagTable", vec![pl::s("Image::ExifTool::Photoshop::ImageData")])?;
            pl::method(it, &et, "ProcessDirectory", vec![di, tag_table_ptr.clone()])?;
        }
        et.set("INDENT", old_indent);
        // process trailers if they exist
        let trail_info = pl::method(it, &et, "IdentifyTrailer", vec![raf.clone()])?;
        if trail_info.truthy() {
            pl::method(it, &et, "ProcessTrailers", vec![trail_info])?;
        }
    }
    ret(rtn_val)
}

/// Register the subs ported so far; the others still run from the Perl.
pub fn register(it: &mut Interp) {
    it.define_native("Image::ExifTool::Photoshop::AUTOLOAD", autoload);
    it.define_native("Image::ExifTool::Photoshop::ConvertPascalString", convert_pascal_string);
    it.define_native("Image::ExifTool::Photoshop::ProcessLayersAndMask", process_layers_and_mask);
    it.define_native("Image::ExifTool::Photoshop::ProcessChannelOptions", process_channel_options);
    it.define_native("Image::ExifTool::Photoshop::ProcessLayers", process_layers);
    it.define_native("Image::ExifTool::Photoshop::ProcessDocumentData", process_document_data);
    it.define_native("Image::ExifTool::Photoshop::ProcessPhotoshop", process_photoshop);
    it.define_native("Image::ExifTool::Photoshop::ProcessPSD", process_psd);
}
