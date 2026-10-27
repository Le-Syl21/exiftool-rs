//! Image::ExifTool::7Z (Image/ExifTool/7Z.pm), ported sub by sub (see docs/port/PORTING.md).
//!
//! Read 7z archive meta information.
//!
//! The Perl's hashes and arrays (%out_packinfo, @out_files...) stay Perl
//! values here, built and read through `perl::hv`, with perl's
//! autovivification where the Perl dereferences in lvalue context (push,
//! foreach, element assignment) and its "Can't use an undefined value"
//! death where it dereferences an undefined value in rvalue context.

use crate::interp::eval::{die, ER};
use crate::interp::{Interp, Want};
use crate::perl::{ops, Value};
use crate::pl::{self, arg, n, ret};

fn s(x: &str) -> Value {
    pl::s(x)
}

fn first(v: Vec<Value>) -> Value {
    v.into_iter().next().unwrap_or_default()
}

/// String interpolation: the parts concatenated.
fn cat(parts: &[Value]) -> Value {
    let mut v = Vec::new();
    for p in parts {
        v.extend_from_slice(&p.to_bytes());
    }
    Value::bytes(v)
}

/// `$raf->Read($buff, $len)`: the count, with `buff` updated.
fn read(it: &mut Interp, raf: &Value, buff: &mut Value, len: Value) -> ER<Value> {
    let mut args = [buff.clone(), len];
    let r = pl::method_mut(it, raf, "Read", &mut args, Want::Scalar)?;
    *buff = args[0].clone();
    Ok(first(r))
}

/// `ord($buff)`
fn ord(v: &Value) -> Value {
    n(v.to_bytes().first().map_or(0, |b| *b as i64))
}

fn vprint(it: &mut Interp, et: &Value, msg: Value) -> ER<()> {
    pl::method(it, et, "VPrint", vec![n(0), msg])?;
    Ok(())
}

fn warn(it: &mut Interp, et: &Value, msg: &str) -> ER<()> {
    pl::method(it, et, "Warn", vec![s(msg)])?;
    Ok(())
}

/// `$i < $count` with a Rust loop counter
fn lt_i(i: i64, v: &Value) -> bool {
    pl::lt(&n(i), v)
}

/// `$x == 0` where `$x` may be a reference (a reference is never 0).
fn is_zero(v: &Value) -> bool {
    !v.is_ref() && pl::num_eq(v, &n(0))
}

/// `@{ EXPR }` in rvalue context (under `use strict`): dies on undef.
fn rv_array(v: &Value) -> ER<Vec<Value>> {
    pl::aderef(v)
}

/// `@{ $$h{key} }` in lvalue context (push, foreach): autovivified.
fn lv_array(h: &Value, key: &str) -> Value {
    h.get_or_new_array(key)
}

/// `$a[$i]->{...}` as an lvalue: the hash in element `i` (autovivified).
fn lv_elem_hash(a: &Value, i: i64) -> Value {
    let e = a.at(i);
    if e.is_hash() {
        return e;
    }
    let h = Value::new_hash();
    a.set_at(i as usize, h.clone());
    h
}

/// `@{ $_[k] }[$i]->{...}`: the hash in element `i` (an element of a
/// slice is not autovivified: perl dies on undef).
fn slice_elem_hash(a: &Value, i: i64) -> ER<Value> {
    let e = a.at(i);
    match &e {
        Value::Undef => die("Can't use an undefined value as a HASH reference"),
        _ => pl::hderef(&e),
    }
}

pub fn read_uint32_rs(it: &mut Interp, raf: &Value) -> ER<Value> {
    let mut buff = Value::Undef;

    pl::at(23);
    read(it, raf, &mut buff, n(4))?;
    let output = first(pl::unpack("L", &buff)?);
    Ok(output)
}

pub fn read_uint32(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    ret(read_uint32_rs(it, &arg(&a, 0))?)
}

pub fn read_uint64_rs(it: &mut Interp, raf: &Value) -> ER<Value> {
    let mut buff = Value::Undef;

    pl::at(32);
    read(it, raf, &mut buff, n(1))?;
    let b = ord(&buff).to_i64();
    if b == 255 {
        // read real uint64
        read(it, raf, &mut buff, n(8))?;
        let output = first(pl::unpack("Q", &buff)?);
        return Ok(output);
    }
    let blen: [i64; 8] = [0x7F, 0xBF, 0xDF, 0xEF, 0xF7, 0xFB, 0xFD, 0xFE];

    let mut mask: i64 = 0x80;
    let mut vlen: i64 = 8;

    for (l, v) in blen.iter().enumerate() {
        if b <= *v {
            vlen = l as i64;
            break;
        }
        mask >>= 1;
    }
    if vlen == 0 {
        return Ok(n(b & (mask - 1)));
    }
    read(it, raf, &mut buff, n(vlen))?;
    buff = pl::concat(&buff, &Value::bytes([0u8; 8]));

    let value = pl::unpack1("Q", &buff)?;
    let highpart = n(b & (mask - 1));
    Ok(pl::add(&value, &ops::shift_left(&highpart, &n(vlen * 8))))
}

pub fn read_uint64(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    ret(read_uint64_rs(it, &arg(&a, 0))?)
}

fn read_real_uint64_rs(it: &mut Interp, raf: &Value) -> ER<Value> {
    let mut buff = Value::Undef;

    pl::at(66);
    read(it, raf, &mut buff, n(8))?;
    let value = pl::unpack1("Q", &buff)?;
    Ok(value)
}

pub fn read_real_uint64(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    ret(read_real_uint64_rs(it, &arg(&a, 0))?)
}

fn read_boolean_rs(it: &mut Interp, raf: &Value, count: &Value, checkall: &Value) -> ER<Vec<Value>> {
    let mut buff = Value::Undef;
    let mut result: Vec<Value> = Vec::new();

    if checkall.truthy() {
        pl::at(78);
        read(it, raf, &mut buff, n(1))?;
        let all_defined = ord(&buff);
        if !pl::num_eq(&all_defined, &n(0)) {
            let c = count.to_i64().max(0) as usize;
            result = vec![n(1); c];
            return Ok(result);
        }
    }

    let mut b: i64 = 0;
    let mut mask: i64 = 0;

    let mut i: i64 = 0;
    while lt_i(i, count) {
        if mask == 0 {
            pl::at(91);
            read(it, raf, &mut buff, n(1))?;
            b = ord(&buff).to_i64();
            mask = 0x80;
        }
        result.push(Value::from((b & mask) != 0));
        mask >>= 1;
        i += 1;
    }
    Ok(result)
}

pub fn read_boolean(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    read_boolean_rs(it, &arg(&a, 0), &arg(&a, 1), &arg(&a, 2))
}

fn read_utf16_rs(it: &mut Interp, raf: &Value) -> ER<Value> {
    let mut val = s("");
    let mut ch = Value::Undef;

    for _ in 0..65536 {
        pl::at(106);
        read(it, raf, &mut ch, n(2))?;
        if pl::str_eq(&ch, "\0\0") {
            break;
        }
        val = pl::concat(&val, &ch);
    }
    Ok(val)
}

pub fn read_utf16(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    ret(read_utf16_rs(it, &arg(&a, 0))?)
}

fn read_pack_info_rs(it: &mut Interp, et: &Value, raf: &Value) -> ER<Value> {
    let mut buff = Value::Undef;
    let out_packinfo = Value::new_hash();
    out_packinfo.set("packsizes", Value::Undef);

    let v = read_uint64_rs(it, raf)?;
    out_packinfo.set("packpos", v);
    let numstreams = read_uint64_rs(it, raf)?;
    vprint(it, et, cat(&[s("Number Of Streams: "), numstreams.clone(), s("\n")]))?;

    read(it, raf, &mut buff, n(1))?;
    let mut pid = ord(&buff).to_i64();

    if pid == 9 {
        // size
        let mut i = 0;
        while lt_i(i, &numstreams) {
            let v = read_uint64_rs(it, raf)?;
            lv_array(&out_packinfo, "packsizes").push(v);
            i += 1;
        }
        read(it, raf, &mut buff, n(1))?;
        pid = ord(&buff).to_i64();
        if pid == 10 {
            // crc
            let mut crcs: Vec<Value> = Vec::new();
            let digestdefined = read_boolean_rs(it, raf, &numstreams, &n(1))?;
            for crcexist in digestdefined {
                if crcexist.truthy() {
                    crcs.push(read_uint32_rs(it, raf)?);
                }
            }
            read(it, raf, &mut buff, n(1))?;
            pid = ord(&buff).to_i64();
        }
    }
    if pid != 0 {
        // end id expected
        return Ok(n(0));
    }
    Ok(out_packinfo)
}

pub fn read_pack_info(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    ret(read_pack_info_rs(it, &arg(&a, 0), &arg(&a, 1))?)
}

fn find_in_bin_pair_rs(bindpairs: &Value, index: &Value) -> ER<Value> {
    pl::at(155);
    let bindpairs = rv_array(bindpairs)?;

    for (i, b) in bindpairs.iter().enumerate() {
        if pl::num_eq(b, index) {
            return Ok(n(i as i64));
        }
    }
    Ok(n(-1))
}

pub fn find_in_bin_pair(_it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    ret(find_in_bin_pair_rs(&arg(&a, 0), &arg(&a, 1))?)
}

fn read_folder_rs(it: &mut Interp, et: &Value, raf: &Value) -> ER<Value> {
    let mut buff = Value::Undef;
    let mut totalin = n(0);
    let mut totalout = n(0);
    let out_folder = Value::new_hash();
    out_folder.set("packed_indices", Value::Undef);
    out_folder.set("bindpairs", Value::Undef);
    out_folder.set("coders", Value::Undef);

    let num_coders = read_uint64_rs(it, raf)?;
    vprint(it, et, cat(&[s("Number of coders: "), num_coders.clone(), s("\n")]))?;

    let mut i = 0;
    while lt_i(i, &num_coders) {
        let c = Value::new_hash();
        read(it, raf, &mut buff, n(1))?;
        let b = ord(&buff).to_i64();
        let methodsize = b & 0xF;
        let iscomplex = (b & 0x10) == 0x10;
        let hasattributes = (b & 0x20) == 0x20;
        if methodsize > 0 {
            read(it, raf, &mut buff, n(methodsize))?;
            c.set("method", buff.clone());
        } else {
            c.set("method", s("\0"));
        }
        if iscomplex {
            let v = read_uint64_rs(it, raf)?;
            c.set("numinstreams", v);
            let v = read_uint64_rs(it, raf)?;
            c.set("numoutstreams", v);
        } else {
            c.set("numinstreams", n(1));
            c.set("numoutstreams", n(1));
        }
        totalin = pl::add(&totalin, &c.get("numinstreams"));
        totalout = pl::add(&totalout, &c.get("numoutstreams"));
        if hasattributes {
            let proplen = read_uint64_rs(it, raf)?;
            read(it, raf, &mut buff, proplen)?;
            c.set("properties", buff.clone());
        } else {
            c.set("properties", Value::Undef);
        }
        vprint(it, et, Value::from(format!("Reading coder {i}\n")))?;
        lv_array(&out_folder, "coders").push(c);
        i += 1;
    }
    let num_bindpairs = pl::sub(&totalout, &n(1));
    let mut i = 0;
    while lt_i(i, &num_bindpairs) {
        let b1 = read_uint64_rs(it, raf)?;
        let b2 = read_uint64_rs(it, raf)?;
        let bp = lv_array(&out_folder, "bindpairs");
        bp.push(b1);
        bp.push(b2);
        i += 1;
    }
    let num_packedstreams = pl::sub(&totalin, &num_bindpairs);
    if pl::num_eq(&num_packedstreams, &n(1)) {
        let mut i = 0;
        while lt_i(i, &totalin) {
            let bp = lv_array(&out_folder, "bindpairs");
            if pl::lt(&find_in_bin_pair_rs(&bp, &n(i))?, &n(0)) {
                lv_array(&out_folder, "packed_indices").push(n(i));
            }
            i += 1;
        }
    } else {
        let mut i = 0;
        while lt_i(i, &num_packedstreams) {
            let v = read_uint64_rs(it, raf)?;
            lv_array(&out_folder, "packed_indices").push(v);
            i += 1;
        }
    }

    Ok(out_folder)
}

pub fn read_folder(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    ret(read_folder_rs(it, &arg(&a, 0), &arg(&a, 1))?)
}

fn retrieve_coders_info_rs(it: &mut Interp, et: &Value, raf: &Value, folders_ref: &Value) -> ER<Value> {
    let mut buff = Value::Undef;
    pl::at(239);
    let folders = rv_array(folders_ref)?;

    read(it, raf, &mut buff, n(1))?;
    let mut pid = ord(&buff).to_i64();

    if pid != 0x0c {
        // coders unpack size id expected
        return Ok(n(0));
    }
    for folder in &folders {
        folder.set("unpacksizes", Value::Undef);
        for c in lv_array(folder, "coders").items() {
            let mut i = 0;
            while lt_i(i, &c.get("numoutstreams")) {
                let v = read_uint64_rs(it, raf)?;
                lv_array(folder, "unpacksizes").push(v);
                i += 1;
            }
        }
    }
    read(it, raf, &mut buff, n(1))?;
    pid = ord(&buff).to_i64();

    if pid == 0x0a {
        //crc
        let numfolders = folders.len() as i64;
        vprint(it, et, Value::from(format!("Number of folders: {numfolders}\n")))?;
        let defined = read_boolean_rs(it, raf, &n(numfolders), &n(1))?;
        let mut crcs: Vec<Value> = Vec::new();
        for crcexist in &defined {
            if crcexist.truthy() {
                crcs.push(read_uint32_rs(it, raf)?);
            }
        }
        for i in 0..numfolders as usize {
            folders[i].set("digestdefined", defined.get(i).cloned().unwrap_or_default());
            folders[i].set("crc", crcs.get(i).cloned().unwrap_or_default());
        }
        read(it, raf, &mut buff, n(1))?;
        pid = ord(&buff).to_i64();
    }

    if pid != 0x00 {
        // end id expected
        vprint(it, et, Value::from(format!("Invalid PID: {pid}\n")))?;
        return Ok(n(0));
    }
    Ok(n(1))
}

pub fn retrieve_coders_info(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    ret(retrieve_coders_info_rs(it, &arg(&a, 0), &arg(&a, 1), &arg(&a, 2))?)
}

fn read_unpack_info_rs(it: &mut Interp, et: &Value, raf: &Value) -> ER<Value> {
    let mut buff = Value::Undef;
    let out_unpackinfo = Value::new_hash();

    read(it, raf, &mut buff, n(1))?;
    let pid = ord(&buff).to_i64();

    if pid != 0xb {
        // folder id expected
        return Ok(n(0));
    }

    let v = read_uint64_rs(it, raf)?;
    out_unpackinfo.set("numfolders", v);
    out_unpackinfo.set("folders", Value::Undef);

    read(it, raf, &mut buff, n(1))?;
    let external = ord(&buff).to_i64();

    if external == 0x00 {
        let mut i = 0;
        while lt_i(i, &out_unpackinfo.get("numfolders")) {
            vprint(it, et, Value::from(format!("Reading folder {i}\n")))?;
            let folder = read_folder_rs(it, et, raf)?;
            lv_array(&out_unpackinfo, "folders").push(folder);
            i += 1;
        }
    }
    if !retrieve_coders_info_rs(it, et, raf, &out_unpackinfo.get("folders"))?.truthy() {
        return Ok(n(0));
    }
    Ok(out_unpackinfo)
}

pub fn read_unpack_info(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    ret(read_unpack_info_rs(it, &arg(&a, 0), &arg(&a, 1))?)
}

fn read_substreams_info_rs(it: &mut Interp, et: &Value, raf: &Value, numfolders: &Value, folders: &Value) -> ER<Value> {
    let mut buff = Value::Undef;
    let out = Value::new_hash();
    out.set("num_unpackstreams_folders", Value::Undef);

    read(it, raf, &mut buff, n(1))?;
    let mut pid = ord(&buff).to_i64();
    if pid == 13 {
        // num unpack stream
        vprint(it, et, s("Num unpack stream detected.\n"))?;
        let mut i = 0;
        while lt_i(i, numfolders) {
            let v = read_uint64_rs(it, raf)?;
            lv_array(&out, "num_unpackstreams_folders").push(v);
            i += 1;
        }
        read(it, raf, &mut buff, n(1))?;
        pid = ord(&buff).to_i64();
    } else {
        let c = numfolders.to_i64().max(0) as usize;
        if let Value::ArrayRef(a) = lv_array(&out, "num_unpackstreams_folders") {
            *a.borrow_mut() = vec![n(1); c];
        }
    }
    if pid == 9 {
        // size property
        vprint(it, et, s("Size property detected.\n"))?;
        out.set("unpacksizes", Value::Undef);
        let mut i: i64 = 0;
        pl::at(337);
        while (i as usize) < rv_array(&out.get("num_unpackstreams_folders"))?.len() {
            let mut totalsize = n(0);
            let mut j: i64 = 1;
            pl::at(339);
            while lt_i(j, &rv_array(&out.get("num_unpackstreams_folders"))?.get(i as usize).cloned().unwrap_or_default()) {
                let size = read_uint64_rs(it, raf)?;
                lv_array(&out, "unpacksizes").push(size.clone());
                totalsize = pl::add(&totalsize, &size);
                j += 1;
                pl::at(339);
            }
            let _ = totalsize;
            // self.unpacksizes.append(folders[i].get_unpack_size() - totalsize)
            i += 1;
            pl::at(337);
        }
        read(it, raf, &mut buff, n(1))?;
        pid = ord(&buff).to_i64();
    }
    let mut num_digests = n(0);
    let mut num_digests_total = n(0);
    let mut i: i64 = 0;
    while lt_i(i, numfolders) {
        pl::at(352);
        let numsubstreams = rv_array(&out.get("num_unpackstreams_folders"))?.get(i as usize).cloned().unwrap_or_default();
        pl::at(353);
        let fi = rv_array(folders)?.get(i as usize).cloned().unwrap_or_default();
        if !pl::num_eq(&numsubstreams, &n(1)) || !fi.get("digestdefined").truthy() {
            num_digests = pl::add(&num_digests, &numsubstreams);
        }
        num_digests_total = pl::add(&num_digests_total, &numsubstreams);
        i += 1;
    }
    vprint(it, et, cat(&[s("Num Digests Total: "), num_digests_total, s("\n")]))?;
    if pid == 10 {
        // crc property
        vprint(it, et, s("CRC property detected.\n"))?;
        let mut crcs: Vec<Value> = Vec::new();
        let defined = read_boolean_rs(it, raf, &num_digests, &n(1))?;
        for _crcexist in defined {
            crcs.push(read_uint32_rs(it, raf)?);
        }
        read(it, raf, &mut buff, n(1))?;
        pid = ord(&buff).to_i64();
    }
    if pid != 0x00 {
        // end id expected
        return Ok(n(0));
    }
    Ok(out)
}

pub fn read_substreams_info(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    ret(read_substreams_info_rs(it, &arg(&a, 0), &arg(&a, 1), &arg(&a, 2), &arg(&a, 3))?)
}

fn read_streams_info_rs(it: &mut Interp, et: &Value, raf: &Value) -> ER<Value> {
    let mut buff = Value::Undef;
    let mut unpackinfo = Value::Undef;
    let out = Value::new_hash();

    read(it, raf, &mut buff, n(1))?;
    let mut pid = ord(&buff).to_i64();
    if pid == 6 {
        // pack info
        let packinfo = read_pack_info_rs(it, et, raf)?;
        if !packinfo.truthy() {
            return Ok(n(0));
        }
        out.set("packinfo", packinfo);
        read(it, raf, &mut buff, n(1))?;
        pid = ord(&buff).to_i64();
    }
    if pid == 7 {
        // unpack info
        vprint(it, et, s("Unpack info data detected.\n"))?;
        unpackinfo = read_unpack_info_rs(it, et, raf)?;
        if !unpackinfo.truthy() {
            return Ok(n(0));
        }
        out.set("unpackinfo", unpackinfo.clone());
        read(it, raf, &mut buff, n(1))?;
        pid = ord(&buff).to_i64();
    }
    if pid == 8 {
        // substreams info
        vprint(it, et, s("Substreams info data detected.\n"))?;
        let substreamsinfo = read_substreams_info_rs(it, et, raf, &unpackinfo.get("numfolders"), &unpackinfo.get("folders"))?;
        if !substreamsinfo.truthy() {
            return Ok(n(0));
        }
        out.set("substreamsinfo", substreamsinfo);
        read(it, raf, &mut buff, n(1))?;
        pid = ord(&buff).to_i64();
    }
    if pid != 0x00 {
        // end id expected
        vprint(it, et, Value::from(format!("Invalid PID: {pid}\n")))?;
        return Ok(n(0));
    }
    Ok(out)
}

pub fn read_streams_info(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    ret(read_streams_info_rs(it, &arg(&a, 0), &arg(&a, 1))?)
}

fn is_native_coder_rs(coder: &Value) -> Value {
    let m = coder.get("method").to_bytes();
    let o = |k: usize| m.get(k).map_or(0, |b| *b as i64); // ord(substr($coder->{"method"}, k, 1))
    if o(0) == 3 {
        if o(1) == 1 {
            if o(2) == 1 {
                return s("LZMA");
            }
        }
    } else if o(0) == 6 {
        if o(1) == 0xf1 {
            if o(2) == 7 {
                if o(3) == 1 {
                    return s("7zAES");
                }
            }
        }
    }
    // (falls off the end: the value of the last condition evaluated, false)
    Value::from(false)
}

pub fn is_native_coder(_it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    ret(is_native_coder_rs(&arg(&a, 0)))
}

fn get_decompressor_rs(it: &mut Interp, et: &Value, folder: &Value, input_size: &Value) -> ER<Value> {
    let out = Value::new_hash();
    out.set("chain", Value::Undef);
    out.set("input_size", input_size.clone());
    out.set("_unpacksizes", folder.get("unpacksizes"));
    pl::at(442);
    let c = rv_array(&out.get("_unpacksizes"))?.len();
    out.set("_unpacked", Value::new_array(vec![n(0); c]));
    out.set("consumed", n(0));
    out.set("block_size", n(32768));
    out.set("_unused", Value::new_array(vec![]));

    for coder in lv_array(folder, "coders").items() {
        let algorithm = is_native_coder_rs(&coder);
        if pl::str_eq(&algorithm, "7zAES") {
            pl::method(it, et, "Warn", vec![s("File is encrypted."), n(0)])?;
            return Ok(n(0));
        } else {
            lv_array(&out, "chain").push(algorithm);
        }
    }

    Ok(out)
}

pub fn get_decompressor(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    ret(get_decompressor_rs(it, &arg(&a, 0), &arg(&a, 1), &arg(&a, 2))?)
}

fn read_data_rs(it: &mut Interp, et: &Value, raf: &Value, decompressor: &Value) -> ER<Value> {
    let rest_size = pl::sub(&decompressor.get("input_size"), &decompressor.get("consumed"));
    pl::at(465);
    let unused_s = n(rv_array(&decompressor.get("_unused"))?.len() as i64);
    let mut read_size = pl::sub(&rest_size, &unused_s);
    let mut data = s("");
    let lim = pl::sub(&decompressor.get("block_size"), &unused_s);
    if pl::gt(&read_size, &lim) {
        read_size = lim;
    }
    if pl::gt(&read_size, &n(0)) {
        let r = read(it, raf, &mut data, read_size.clone())?;
        decompressor.set("consumed", pl::add(&decompressor.get("consumed"), &r));
        vprint(it, et, cat(&[s("Compressed size: "), read_size, s("\n")]))?;
    }
    Ok(data)
}

pub fn read_data(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    ret(read_data_rs(it, &arg(&a, 0), &arg(&a, 1), &arg(&a, 2))?)
}

fn decompress_internal_rs(it: &mut Interp, decompressor: &Value, input: &Value) -> ER<Value> {
    let mut data = s("");
    let mut input = input.clone();
    let mut i: usize = 0;
    pl::at(480);
    while i < rv_array(&decompressor.get("chain"))?.len() {
        pl::at(481);
        let unpacked = rv_array(&decompressor.get("_unpacked"))?.get(i).cloned().unwrap_or_default();
        let unpacksize = rv_array(&decompressor.get("_unpacksizes"))?.get(i).cloned().unwrap_or_default();
        if pl::lt(&unpacked, &unpacksize) {
            let opts = Value::new_hash();
            opts.set("Filter", pl::call(it, "Lzma::Filter::Lzma1", vec![])?);
            let mut args = vec![];
            for k in opts.keys() {
                args.push(Value::bytes(&k[..]));
                args.push(opts.get(&k[..]));
            }
            let r = pl::method_list(it, &s("Compress::Raw::Lzma::RawDecoder"), "new", args)?;
            let z = arg(&r, 0);
            let mut cargs = [input.clone(), data.clone()];
            pl::method_mut(it, &z, "code", &mut cargs, Want::Scalar)?;
            input = cargs[0].clone();
            data = cargs[1].clone();
            let u = lv_array(decompressor, "_unpacked");
            let nv = pl::add(&u.at(i as i64), &pl::length(&data));
            u.set_at(i, nv);
        }
        i += 1;
        pl::at(480);
    }
    Ok(data)
}

pub fn decompress_internal(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let r = decompress_internal_rs(it, &arg(&a, 0), &arg(&a, 1))?;
    ret(r)
}

fn decompress_rs(it: &mut Interp, et: &Value, raf: &Value, decompressor: &Value) -> ER<Value> {
    let data = read_data_rs(it, et, raf, decompressor)?;
    let tmp = decompress_internal_rs(it, decompressor, &data)?;
    Ok(tmp)
}

pub fn decompress(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    // (my $max_length = $_[1]; unused)
    ret(decompress_rs(it, &arg(&a, 0), &arg(&a, 1), &arg(&a, 2))?)
}

fn read_name_rs(it: &mut Interp, raf: &Value, numfiles: &Value, files: &Value) -> ER<()> {
    let mut i = 0;
    while lt_i(i, numfiles) {
        let v = read_utf16_rs(it, raf)?;
        pl::at(503);
        slice_elem_hash(files, i)?.set("filename", v);
        i += 1;
    }
    Ok(())
}

pub fn read_name(it: &mut Interp, a: Vec<Value>, want: Want) -> ER<Vec<Value>> {
    read_name_rs(it, &arg(&a, 0), &arg(&a, 1), &arg(&a, 2))?;
    pl::ret_nothing(want)
}

fn read_times_rs(it: &mut Interp, et: &Value, raf: &Value, numfiles: &Value, name: &Value, files: &Value) -> ER<Value> {
    let mut external = Value::Undef;

    let defined = read_boolean_rs(it, raf, numfiles, &n(1))?;
    read(it, raf, &mut external, n(1))?;
    if !pl::num_eq(&ord(&external), &n(0)) {
        warn(it, et, "Invalid or corrupted file. (ReadTimes)")?;
        return Ok(n(0));
    }

    let mut i = 0;
    while lt_i(i, numfiles) {
        if defined.get(i as usize).is_some_and(|d| d.truthy()) {
            let mut value = read_real_uint64_rs(it, raf)?;
            value = pl::sub(&pl::div(&value, &Value::Num(10000000.0))?, &n(11644473600));
            pl::at(525);
            slice_elem_hash(files, i)?.set(name.to_bytes(), value);
        } else {
            pl::at(528);
            slice_elem_hash(files, i)?.set(name.to_bytes(), Value::Undef);
        }
        i += 1;
    }
    Ok(Value::Undef)
}

pub fn read_times(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    ret(read_times_rs(it, &arg(&a, 0), &arg(&a, 1), &arg(&a, 2), &arg(&a, 3), &arg(&a, 4))?)
}

fn read_attributes_rs(it: &mut Interp, raf: &Value, numfiles: &Value, defined: &Value, files: &Value) -> ER<()> {
    let mut i = 0;
    while lt_i(i, numfiles) {
        if defined.at(i).truthy() {
            let value = read_uint32_rs(it, raf)?;
            pl::at(539);
            slice_elem_hash(files, i)?.set("attributes", ops::shift_right(&value, &n(8)));
        } else {
            pl::at(542);
            slice_elem_hash(files, i)?.set("attributes", Value::Undef);
        }
        i += 1;
    }
    Ok(())
}

pub fn read_attributes(it: &mut Interp, a: Vec<Value>, want: Want) -> ER<Vec<Value>> {
    read_attributes_rs(it, &arg(&a, 0), &arg(&a, 1), &arg(&a, 2), &arg(&a, 3))?;
    pl::ret_nothing(want)
}

fn read_files_info_rs(it: &mut Interp, et: &Value, raf: &Value) -> ER<Value> {
    let mut buff = Value::Undef;

    let numfiles = read_uint64_rs(it, raf)?;
    let out_files = Value::new_array(vec![]);
    let mut i = 0;
    while lt_i(i, &numfiles) {
        let new_file = Value::new_hash();
        new_file.set("emptystream", n(0));
        out_files.push(new_file);
        i += 1;
    }
    vprint(it, et, cat(&[s("Number of files: "), numfiles.clone(), s("\n")]))?;
    loop {
        read(it, raf, &mut buff, n(1))?;
        let prop = ord(&buff).to_i64();
        if prop == 0 {
            // end
            return Ok(out_files);
        }
        let size = read_uint64_rs(it, raf)?;
        if prop == 25 {
            // dummy
            pl::method(it, raf, "Seek", vec![size, n(1)])?;
            continue;
        }
        read(it, raf, &mut buff, size)?;
        let buffer = pl::method(it, &s("File::RandomAccess"), "new", vec![pl::sref(buff.clone())])?;
        if prop == 14 {
            // empty stream
            let isempty = read_boolean_rs(it, &buffer, &numfiles, &n(0))?;
            let mut numemptystreams = 0;
            let mut i = 0;
            while lt_i(i, &numfiles) {
                if pl::num_eq(&isempty.get(i as usize).cloned().unwrap_or_default(), &n(0)) {
                    lv_elem_hash(&out_files, i).set("emptystream", n(0));
                } else {
                    lv_elem_hash(&out_files, i).set("emptystream", n(1));
                    numemptystreams += 1;
                }
                i += 1;
            }
            let _ = numemptystreams;
        } else if prop == 15 {
            // empty file
        } else if prop == 17 {
            // name
            vprint(it, et, s("Name prop detected.\n"))?;
            let mut external = Value::Undef;
            read(it, &buffer, &mut external, n(1))?;
            let is_external = ord(&external).to_i64();
            if is_external == 0 {
                read_name_rs(it, &buffer, &numfiles, &out_files)?;
            }
        } else if prop == 20 {
            // last write time
            vprint(it, et, s("Last write time detected.\n"))?;
            read_times_rs(it, et, &buffer, &numfiles, &s("lastwritetime"), &out_files)?;
        } else if prop == 21 {
            // attributes
            vprint(it, et, s("File attributes detected.\n"))?;
            let mut external = Value::Undef;
            let defined = read_boolean_rs(it, &buffer, &numfiles, &n(1))?;
            read(it, raf, &mut external, n(1))?;
            if ord(&external).to_i64() == 0 {
                read_attributes_rs(it, &buffer, &numfiles, &Value::new_array(defined), &out_files)?;
            } else {
                // (ReadUINT64 is not defined: perl dies here)
                let _dataindex = pl::call(it, "Image::ExifTool::7Z::ReadUINT64", vec![buffer.clone()])?;
                //TODO: try to read external data
            }
        }
    }
}

pub fn read_files_info(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    ret(read_files_info_rs(it, &arg(&a, 0), &arg(&a, 1))?)
}

fn extract_header_info_rs(it: &mut Interp, et: &Value, raf: &Value) -> ER<Value> {
    let mut buff = Value::Undef;
    let out = Value::new_hash();
    out.set("files_info", Value::Undef);

    read(it, raf, &mut buff, n(1))?;
    let mut pid = ord(&buff).to_i64();

    if pid == 0x04 {
        let mainstreams = read_streams_info_rs(it, et, raf)?;
        if is_zero(&mainstreams) {
            warn(it, et, "Invalid or corrupted file. (ExtractHeaderInfo)")?;
            return Ok(n(0));
        }
        read(it, raf, &mut buff, n(1))?;
        pid = ord(&buff).to_i64();
    }
    if pid == 0x05 {
        vprint(it, et, s("File info pid reached.\n"))?;
        let files_info = read_files_info_rs(it, et, raf)?;
        lv_array(&out, "files_info").push(files_info);
        read(it, raf, &mut buff, n(1))?;
        pid = ord(&buff).to_i64();
    }
    if pid != 0x00 {
        // end id expected
        vprint(it, et, Value::from(format!("Invalid PID: {pid}\n")))?;
        return Ok(n(0));
    }
    Ok(out)
}

pub fn extract_header_info(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    ret(extract_header_info_rs(it, &arg(&a, 0), &arg(&a, 1))?)
}

fn display_files_rs(it: &mut Interp, et: &Value, files: &Value) -> ER<()> {
    let mut doc_num: i64 = 0;
    let tag_table_ptr = pl::call(it, "Image::ExifTool::GetTagTable", vec![s("Image::ExifTool::ZIP::RAR5")])?;

    // (foreach autovivifies an undefined $_[0])
    pl::at(656);
    let list = if files.is_undef() { Vec::new() } else { pl::aderef(files)? };
    for currentfile in list {
        doc_num += 1;
        et.set("DOC_NUM", n(doc_num));
        pl::method(it, et, "HandleTag", vec![tag_table_ptr.clone(), s("ModifyDate"), currentfile.get("lastwritetime")])?;
        pl::method(it, et, "HandleTag", vec![tag_table_ptr.clone(), s("ArchivedFileName"), currentfile.get("filename")])?;
    }
    et.delete("DOC_NUM");
    if doc_num > 1 && !pl::method(it, et, "Options", vec![s("Duplicates")])?.truthy() {
        pl::method(it, et, "Warn", vec![Value::from(format!("Use the Duplicates option to extract tags for all {doc_num} files")), n(1)])?;
    }
    Ok(())
}

pub fn display_files(it: &mut Interp, a: Vec<Value>, want: Want) -> ER<Vec<Value>> {
    display_files_rs(it, &arg(&a, 0), &arg(&a, 1))?;
    pl::ret_nothing(want)
}

//------------------------------------------------------------------------------
// Extract information from a 7z file
// Inputs: 0) ExifTool object reference, 1) dirInfo reference
// Returns: 1 on success, 0 if this wasn't a valid 7z file
pub fn process_7z(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info) = (arg(&a, 0), arg(&a, 1));
    let raf = dir_info.get("RAF");
    let mut buff = Value::Undef;

    pl::at(677);
    if !(read(it, &raf, &mut buff, n(6))?.truthy() && buff.to_bytes()[..] == b"7z\xbc\xaf\x27\x1c"[..]) {
        return ret(n(0));
    }

    pl::method(it, &et, "SetFileType", vec![])?;

    read(it, &raf, &mut buff, n(2))?;
    let u = pl::unpack("cc", &buff)?;
    let (major_version, minor_version) = (arg(&u, 0), arg(&u, 1));
    let tag_table_ptr = pl::call(it, "Image::ExifTool::GetTagTable", vec![s("Image::ExifTool::ZIP::RAR5")])?;
    pl::method(it, &et, "HandleTag", vec![tag_table_ptr.clone(), s("FileVersion"), pl::sprintf("7z v%d.%.2d", &[major_version, minor_version])])?;

    pl::method(it, &raf, "Seek", vec![n(4), n(1)])?; // skip Start Header CRC

    read(it, &raf, &mut buff, n(20))?;
    let u = pl::unpack("QQx", &buff)?;
    let (nextheaderoffset, nextheadersize) = (arg(&u, 0), arg(&u, 1));
    vprint(it, &et, cat(&[s("NextHeaderOffset: "), nextheaderoffset.clone(), s(", NextHeaderSize: "), nextheadersize, s("\n")]))?;

    pl::method(it, &raf, "Seek", vec![nextheaderoffset, n(1)])?; // going to next header offset
    read(it, &raf, &mut buff, n(1))?;
    let mut pid = ord(&buff).to_i64();
    if pid == 1 {
        // normal header
        vprint(it, &et, s("Normal header detected. trying to decode\n"))?;
        let headerinfo = extract_header_info_rs(it, &et, &raf)?;
        if is_zero(&headerinfo) {
            warn(it, &et, "Invalid or corrupted file.")?;
            return ret(n(1));
        }
        display_files_rs(it, &et, &lv_array(&headerinfo, "files_info").at(0))?;
    } else if pid == 23 {
        // encoded header
        if pl::eval_block(it, |it| crate::runtime::require(it, "Compress::Raw::Lzma"))?.is_none() {
            warn(it, &et, "Install Compress::Raw::Lzma to read encoded 7z information")?;
            return ret(n(1));
        }
        vprint(it, &et, s("Encoded Header detected. trying to decode\n"))?;
        let streamsinfo = read_streams_info_rs(it, &et, &raf)?;
        if is_zero(&streamsinfo) {
            warn(it, &et, "Invalid or corrupted file.")?;
            return ret(n(1));
        }
        let mut buffer2 = Value::Undef;
        let unpackinfo = streamsinfo.get_or_new_hash("unpackinfo");
        for folder in lv_array(&unpackinfo, "folders").items() {
            pl::at(717);
            let uncompressed = rv_array(&folder.get("unpacksizes"))?;
            let compressed_size = streamsinfo.get("packinfo").get("packsizes").at(0);
            let uncompressed_size = Value::new_array(uncompressed.clone()).at(uncompressed.len() as i64 - 1);
            let decomporessor = get_decompressor_rs(it, &et, &folder, &compressed_size)?;
            if is_zero(&decomporessor) {
                warn(it, &et, "Invalid or corrupted file.")?;
                return ret(n(1));
            }

            let mut src_start = n(32);
            src_start = pl::add(&src_start, &streamsinfo.get("packinfo").get("packpos"));
            pl::method(it, &raf, "Seek", vec![src_start, n(0)])?;
            let mut remaining = uncompressed_size.clone();
            let mut folder_data = s("");
            while pl::gt(&remaining, &n(0)) {
                let d = decompress_rs(it, &et, &raf, &decomporessor)?;
                folder_data = pl::concat(&folder_data, &d);
                remaining = pl::sub(&uncompressed_size, &pl::length(&folder_data));
            }
            buffer2 = pl::method(it, &s("File::RandomAccess"), "new", vec![pl::sref(folder_data)])?;
        }
        pl::method(it, &buffer2, "Seek", vec![n(0), n(0)])?;
        read(it, &buffer2, &mut buff, n(1))?;
        pid = ord(&buff).to_i64();
        if pid != 0x01 {
            // header field expected
            return ret(n(0));
        }
        let headerinfo = extract_header_info_rs(it, &et, &buffer2)?;
        if is_zero(&headerinfo) {
            warn(it, &et, "Invalid or corrupted file.")?;
            return ret(n(1));
        }
        display_files_rs(it, &et, &lv_array(&headerinfo, "files_info").at(0))?;
    } else {
        // Unknown header
        return ret(n(0));
    }

    ret(n(1))
}

pub fn register(it: &mut Interp) {
    it.define_native("Image::ExifTool::7Z::ReadUInt32", read_uint32);
    it.define_native("Image::ExifTool::7Z::ReadUInt64", read_uint64);
    it.define_native("Image::ExifTool::7Z::ReadRealUInt64", read_real_uint64);
    it.define_native("Image::ExifTool::7Z::ReadBoolean", read_boolean);
    it.define_native("Image::ExifTool::7Z::ReadUTF16", read_utf16);
    it.define_native("Image::ExifTool::7Z::ReadPackInfo", read_pack_info);
    it.define_native("Image::ExifTool::7Z::findInBinPair", find_in_bin_pair);
    it.define_native("Image::ExifTool::7Z::ReadFolder", read_folder);
    it.define_native("Image::ExifTool::7Z::RetrieveCodersInfo", retrieve_coders_info);
    it.define_native("Image::ExifTool::7Z::ReadUnpackInfo", read_unpack_info);
    it.define_native("Image::ExifTool::7Z::ReadSubstreamsInfo", read_substreams_info);
    it.define_native("Image::ExifTool::7Z::ReadStreamsInfo", read_streams_info);
    it.define_native("Image::ExifTool::7Z::IsNativeCoder", is_native_coder);
    it.define_native("Image::ExifTool::7Z::GetDecompressor", get_decompressor);
    it.define_native("Image::ExifTool::7Z::ReadData", read_data);
    it.define_native("Image::ExifTool::7Z::Decompress_Internal", decompress_internal);
    it.define_native("Image::ExifTool::7Z::Decompress", decompress);
    it.define_native("Image::ExifTool::7Z::ReadName", read_name);
    it.define_native("Image::ExifTool::7Z::ReadTimes", read_times);
    it.define_native("Image::ExifTool::7Z::ReadAttributes", read_attributes);
    it.define_native("Image::ExifTool::7Z::ReadFilesInfo", read_files_info);
    it.define_native("Image::ExifTool::7Z::ExtractHeaderInfo", extract_header_info);
    it.define_native("Image::ExifTool::7Z::DisplayFiles", display_files);
    it.define_native("Image::ExifTool::7Z::Process7Z", process_7z);
}
