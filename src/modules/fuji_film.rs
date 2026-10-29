//! Image::ExifTool::FujiFilm (Image/ExifTool/FujiFilm.pm), ported sub by sub (see docs/port/PORTING.md).
//!
//! Read/write FujiFilm maker notes and RAF images

use crate::interp::eval::{Cell, ER};
use crate::interp::{Interp, Want};
use crate::modules::random_access::{cell, raf_read_cell, substr};
use crate::perl::{ops, Value};
use crate::pl::{self, arg, cap, n, re, ret};

const MODULE: &str = "Image::ExifTool::FujiFilm";

/// `Image::ExifTool::NAME(ARGS)` (the functions imported with :DataAccess :Utils).
fn call(it: &mut Interp, name: &str, args: Vec<Value>) -> ER<Value> {
    pl::call(it, &format!("Image::ExifTool::{name}"), args)
}

fn hash(pairs: Vec<(&str, Value)>) -> Value {
    let h = Value::new_hash();
    assign(&h, pairs);
    h
}

/// `%h = ( KEY => VAL, ... )` on an existing hash (others may hold `\%h`)
fn assign(h: &Value, pairs: Vec<(&str, Value)>) {
    for k in h.keys() {
        h.delete(&k[..]);
    }
    for (k, v) in pairs {
        h.set(k, v);
    }
}

fn or(a: Value, b: impl FnOnce() -> Value) -> Value {
    if a.truthy() {
        a
    } else {
        b()
    }
}

fn val(c: &Cell) -> Value {
    c.borrow().clone()
}

fn rf(c: &Cell) -> Value {
    Value::ScalarRef(c.clone())
}

fn cat(parts: &[Value]) -> Value {
    let mut v = Vec::new();
    for p in parts {
        v.extend_from_slice(&p.to_bytes());
    }
    Value::bytes(v)
}

fn read(it: &mut Interp, raf: &Value, buff: &Cell, len: Value) -> ER<Value> {
    raf_read_cell(it, raf, buff, len)
}

fn seek(it: &mut Interp, raf: &Value, pos: Value, whence: i64) -> ER<bool> {
    Ok(pl::method(it, raf, "Seek", vec![pos, n(whence)])?.truthy())
}

fn error(it: &mut Interp, et: &Value, msg: &str) -> ER<Value> {
    pl::method(it, et, "Error", vec![pl::s(msg)])
}

fn write(it: &mut Interp, outfile: &Value, data: Vec<Value>) -> ER<bool> {
    let mut a = vec![outfile.clone()];
    a.extend(data);
    Ok(call(it, "Write", a)?.truthy())
}

/// `File::RandomAccess->new(\$buff)`
fn new_raf(it: &mut Interp, data_ref: Value) -> ER<Value> {
    pl::method(it, &pl::s("File::RandomAccess"), "new", vec![data_ref])
}

//------------------------------------------------------------------------------
/// decode information from FujiFilm face recognition information
/// Inputs: 0) ExifTool object reference, 1) dirInfo reference, 2) tag table ref
/// Returns: 1
pub fn process_face_rec(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info, tag_table_ptr) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    let data_pt = dir_info.get("DataPt");
    let data_pos = pl::add(&dir_info.get("DataPos"), &or(dir_info.get("Base"), || n(0)));
    let dir_start = dir_info.get("DirStart");
    let dir_len = dir_info.get("DirLen");
    let mut pos = dir_start.clone();
    let end = pl::add(&dir_start, &dir_len);
    pl::method(it, &et, "VerboseDir", vec![pl::s("FaceRecInfo")])?;
    let mut i = n(1);
    loop {
        if pl::gt(&pl::add(&pos, &n(8)), &end) {
            break;
        }
        let off = pl::add(&call(it, "Get32u", vec![data_pt.clone(), pos.clone()])?, &dir_start);
        let len = call(it, "Get32u", vec![data_pt.clone(), pl::add(&pos, &n(4))])?;
        if pl::num_eq(&len, &n(0)) || pl::gt(&off, &end) || pl::gt(&pl::add(&off, &len), &end) || pl::lt(&len, &n(62)) {
            break;
        }
        // values observed for each offset (always zero if not listed):
        // 0=5; 3=1; 4=4; 6=1; 10-13=numbers(constant for a given registered face)
        // 15=16; 16=3; 18=1; 22=nameLen; 26=1; 27=16; 28=7; 30-33=nameLen(int32u)
        // 34-37=nameOffset(int32u); 38=32; 39=16; 40=4; 42=1; 46=0,2,4,8(category)
        // 50=33; 51=16; 52=7; 54-57=dateLen(int32u); 58-61=dateOffset(int32u)
        let mut nn = call(it, "Get32u", vec![data_pt.clone(), pl::add(&off, &n(30))])?;
        let mut p = pl::add(&call(it, "Get32u", vec![data_pt.clone(), pl::add(&off, &n(34))])?, &dir_start);
        if pl::lt(&p, &dir_start) || pl::gt(&pl::add(&p, &nn), &end) {
            break;
        }
        pl::at(1662);
        let mut v = substr(it, &pl::sderef(&data_pt)?, &p, Some(&nn))?;
        let parms = |p: &Value, nn: &Value| vec![pl::s("DataPt"), data_pt.clone(), pl::s("DataPos"), data_pos.clone(), pl::s("Start"), p.clone(), pl::s("Size"), nn.clone()];
        let mut args = vec![tag_table_ptr.clone(), cat(&[pl::s("Face"), i.clone(), pl::s("Name")]), v];
        args.extend(parms(&p, &nn));
        pl::method(it, &et, "HandleTag", args)?;
        nn = call(it, "Get32u", vec![data_pt.clone(), pl::add(&off, &n(54))])?;
        p = pl::add(&call(it, "Get32u", vec![data_pt.clone(), pl::add(&off, &n(58))])?, &dir_start);
        if pl::lt(&p, &dir_start) || pl::gt(&pl::add(&p, &nn), &end) {
            break;
        }
        pl::at(1672);
        v = substr(it, &pl::sderef(&data_pt)?, &p, Some(&nn))?;
        re(r"(\d{4})(\d{2})(\d{2})", "").subst(&mut v, false, |c| cat(&[cap(c, 1), pl::s(":"), cap(c, 2), pl::s(":"), cap(c, 2)]).to_bytes().to_vec());
        let mut args = vec![tag_table_ptr.clone(), cat(&[pl::s("Face"), i.clone(), pl::s("Birthday")]), v];
        args.extend(parms(&p, &nn));
        pl::method(it, &et, "HandleTag", args)?;
        let mut args = vec![tag_table_ptr.clone(), cat(&[pl::s("Face"), i.clone(), pl::s("Category")]), Value::Undef];
        args.extend(parms(&pl::add(&off, &n(46)), &n(1)));
        pl::method(it, &et, "HandleTag", args)?;
        pos = pl::add(&pos, &n(8));
        i = pl::add(&i, &n(1));
    }
    ret(n(1))
}

//------------------------------------------------------------------------------
/// get information from FujiFilm RAF directory
/// Inputs: 0) ExifTool object reference, 1) dirInfo reference, 2) tag table ref
/// Returns: 1 if this was a valid FujiFilm directory
pub fn process_fuji_dir(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info, tag_table_ptr) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    let raf = dir_info.get("RAF");
    let offset = dir_info.get("DirStart");
    if !seek(it, &raf, offset.clone(), 0)? {
        return ret(n(0));
    }
    let buff = cell(Value::Undef);
    if !read(it, &raf, &buff, n(4))?.truthy() {
        return ret(n(0));
    }
    let entries = pl::unpack1("N", &val(&buff))?;
    if !pl::lt(&entries, &n(256)) {
        return ret(n(0));
    }
    pl::method(it, &et, "VerboseDir", vec![pl::s("Fuji"), entries.clone()])?;
    call(it, "SetByteOrder", vec![pl::s("MM")])?;
    let mut pos = pl::add(&offset, &n(4));
    let mut index = n(0);
    while pl::lt(&index, &entries) {
        'entry: {
            if !read(it, &raf, &buff, n(4))?.truthy() {
                return ret(n(0));
            }
            pos = pl::add(&pos, &n(4));
            let u = pl::unpack("nn", &val(&buff))?;
            let (tag, len) = (arg(&u, 0), arg(&u, 1));
            let vbuf = cell(Value::Undef);
            if !read(it, &raf, &vbuf, len.clone())?.truthy() {
                return ret(n(0));
            }
            let tag_info = pl::method(it, &et, "GetTagInfo", vec![tag_table_ptr.clone(), tag.clone()])?;
            let v;
            if tag_info.truthy() && tag_info.get("Format").truthy() {
                v = call(it, "ReadValue", vec![rf(&vbuf), n(0), tag_info.get("Format"), tag_info.get("Count"), len.clone()])?;
                if v.is_undef() {
                    break 'entry;
                }
            } else if pl::num_eq(&len, &n(4)) {
                // interpret unknown 4-byte values as int32u
                v = call(it, "Get32u", vec![rf(&vbuf), n(0)])?;
            } else {
                // treat other unknown values as binary data
                v = rf(&vbuf);
            }
            pl::method(
                it,
                &et,
                "HandleTag",
                vec![tag_table_ptr.clone(), tag, v, pl::s("Index"), index.clone(), pl::s("DataPt"), rf(&vbuf), pl::s("DataPos"), pos.clone(), pl::s("Size"), len.clone(), pl::s("TagInfo"), tag_info],
            )?;
            pos = pl::add(&pos, &len);
        }
        index = pl::add(&index, &n(1));
    }
    ret(n(1))
}

//------------------------------------------------------------------------------
/// get information from FujiFilm M-RAW header
/// Inputs: 0) ExifTool ref, 1) dirInfo ref, 2) tag table ref
/// Returns: 1 if this was a valid M-RAW header
pub fn process_mraw(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info, tag_table_ptr) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    if et.get("DOC_NUM").truthy() {
        return ret(n(1));
    }
    let data_pt = dir_info.get("DataPt");
    let data_pos = dir_info.get("DataPos");
    pl::at(1747);
    let data_len = pl::length(&pl::sderef(&data_pt)?);
    if pl::lt(&data_len, &n(44)) {
        pl::method(it, &et, "Warn", vec![pl::s("Short M-RAW header")])?;
        return ret(n(0));
    }
    if !re(r"^FUJIFILMM-RAW  ", "").is_match(&data_pt.deref()) {
        pl::method(it, &et, "Warn", vec![pl::s("Bad M-RAW header")])?;
        return ret(n(0));
    }
    let ver = substr(it, &data_pt.deref(), &n(16), Some(&n(4)))?;
    pl::method(it, &et, "VerboseDir", vec![pl::concat(&pl::s("M-RAW "), &ver), Value::Undef, data_len.clone()])?;
    call(it, "SetByteOrder", vec![pl::s("MM")])?;
    let size = call(it, "Get16u", vec![data_pt.clone(), n(40)])?; // (these are just a guess - PH)
    let num = call(it, "Get16u", vec![data_pt.clone(), n(42)])?;
    let mut pos = n(44);
    let mut nn = n(0);
    loop {
        'image: {
            let end = pl::add(&pl::add(&pos, &n(16)), &size);
            if pl::gt(&end, &data_len) {
                et.delete("DOC_NUM");
                return ret(n(1));
            }
            let raf_start = call(it, "Get64u", vec![data_pt.clone(), pos.clone()])?;
            let raf_len = call(it, "Get64u", vec![data_pt.clone(), pl::add(&pos, &n(8))])?;
            pos = pl::add(&pos, &n(16)); // skip offset/size fields
            if pl::gt(&pos, &n(60)) {
                let dc = pl::add(&et.get("DOC_COUNT"), &n(1));
                et.set("DOC_COUNT", dc.clone());
                et.set("DOC_NUM", dc);
            }
            let msg = cat(&[et.get("INDENT"), pl::s("(Raw image "), nn.clone(), pl::s(" parameters: "), size.clone(), pl::s(" bytes, "), num.clone(), pl::s(" entries)\n")]);
            pl::method(it, &et, "VPrint", vec![n(0), msg])?;
            let mut i = n(0);
            while pl::lt(&i, &num) {
                if pl::gt(&pl::add(&pos, &n(4)), &end) {
                    break;
                }
                let tag = call(it, "Get16u", vec![data_pt.clone(), pos.clone()])?;
                let size = call(it, "Get16u", vec![data_pt.clone(), pl::add(&pos, &n(2))])?;
                pos = pl::add(&pos, &n(4));
                if pl::gt(&pl::add(&pos, &size), &end) {
                    break;
                }
                pl::method(
                    it,
                    &et,
                    "HandleTag",
                    vec![tag_table_ptr.clone(), tag, Value::Undef, pl::s("DataPt"), data_pt.clone(), pl::s("DataPos"), data_pos.clone(), pl::s("Start"), pos.clone(), pl::s("Size"), size.clone()],
                )?;
                pos = pl::add(&pos, &size);
                i = pl::add(&i, &n(1));
            }
            if raf_start.truthy() && pl::method(it, &et, "Options", vec![pl::s("ExtractEmbedded")])?.truthy() {
                if pl::method(it, &et, "Options", vec![pl::s("Verbose")])?.truthy() {
                    let fmt = pl::concat(&et.get("INDENT"), &pl::s("(RAW image "));
                    let fmt = cat(&[fmt, nn.clone(), pl::s(" data: Start=0x%x, Length=0x%x)\n")]);
                    let msg = pl::sprintf_v(&fmt, &[raf_start.clone(), raf_len.clone()]);
                    pl::method(it, &et, "VPrint", vec![n(0), msg])?;
                }
                let raf = et.get("RAF");
                let tell = pl::method(it, &raf, "Tell", vec![])?;
                let order = call(it, "GetByteOrder", vec![])?;
                let fuji_width = et.get("FujiWidth");
                if !seek(it, &raf, raf_start.clone(), 0)? {
                    break 'image;
                }
                process_raf(it, vec![et.clone(), hash(vec![("RAF", raf.clone()), ("Base", raf_start.clone())])], Want::Scalar)?;
                et.set("FujiWidth", fuji_width);
                call(it, "SetByteOrder", vec![order])?;
                pl::method(it, &raf, "Seek", vec![tell, n(0)])?;
            }
        }
        nn = pl::add(&nn, &n(1));
    }
}

//------------------------------------------------------------------------------
/// write information to FujiFilm RAW file (RAF)
/// Inputs: 0) ExifTool object reference, 1) dirInfo reference
/// Returns: 1 on success, 0 if this wasn't a valid RAF file, or -1 on write error
pub fn write_raf(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info) = (arg(&a, 0), arg(&a, 1));
    let raf = dir_info.get("RAF");
    let (hdr, jpeg, out_jpeg, buff) = (cell(Value::Undef), cell(Value::Undef), cell(Value::Undef), cell(Value::Undef));
    let mut err = false;

    if !pl::num_eq(&read(it, &raf, &hdr, n(0x94))?, &n(0x94)) {
        return ret(n(0));
    }
    if !re(r"^FUJIFILM", "").is_match(&val(&hdr)) {
        return ret(n(0));
    }
    let ver = substr(it, &val(&hdr), &n(0x3c), Some(&n(4)))?;
    let tested_raf = crate::runtime::lex_hash(it, MODULE, "testedRAF");
    if !(re(r"^\d{4}$", "").is_match(&ver) || tested_raf.get(ver.to_bytes()).truthy()) {
        return ret(n(0));
    }

    // get position and size of M-RAW header
    let u = pl::unpack("x72NN", &val(&hdr))?;
    let (mpos, mlen) = (arg(&u, 0), arg(&u, 1));
    // get the position and size of embedded JPEG
    let u = pl::unpack("x84NN", &val(&hdr))?;
    let (jpos, jlen) = (arg(&u, 0), arg(&u, 1));
    // check to be sure the JPEG starts in the expected location
    if (pl::gt(&mpos, &n(0x94)) || pl::gt(&jpos, &pl::add(&n(0x94), &mlen))) || pl::lt(&jpos, &n(0x68)) || ops::bit_and(&jpos, &n(0x03)).truthy() {
        let msg = cat(&[pl::s("Unsupported or corrupted RAF image (version "), ver.clone(), pl::s(")")]);
        pl::method(it, &et, "Error", vec![msg])?;
        return ret(n(1));
    }
    // check to make sure this version of RAF has been tested
    //(removed in ExifTool 11.70)
    //unless ($testedRAF{$ver}) {
    //    $et->Warn("RAF version $ver not yet tested", 1);
    //}
    // read the embedded JPEG
    if !(seek(it, &raf, jpos.clone(), 0)? && pl::num_eq(&read(it, &raf, &jpeg, jlen.clone())?, &jlen)) {
        error(it, &et, "Error reading RAF meta information")?;
        return ret(n(1));
    }
    if mpos.truthy() {
        if !pl::num_eq(&mlen, &n(0x11c)) {
            error(it, &et, "Unsupported M-RAW header (please submit sample for testing)")?;
            return ret(n(1));
        }
        // read M-RAW header and add to file header
        let mraw = cell(Value::Undef);
        if !(seek(it, &raf, mpos.clone(), 0)? && pl::num_eq(&read(it, &raf, &mraw, mlen.clone())?, &mlen)) {
            error(it, &et, "Error reading M-RAW header")?;
            return ret(n(1));
        }
        let h = pl::concat(&val(&hdr), &val(&mraw));
        *hdr.borrow_mut() = h;
        // verify that the 1st raw image offset is zero, and that the 1st raw image
        // length is the same as the 2nd raw image offset
        let h = val(&hdr);
        let a1 = substr(it, &h, &n(0xc0), Some(&n(8)))?;
        let ok = pl::str_eq(&a1, "\0\0\0\0\0\0\0\0") && {
            let b1 = substr(it, &h, &n(0xc8), Some(&n(8)))?;
            let b2 = substr(it, &h, &n(0x110), Some(&n(8)))?;
            b1.to_bytes() == b2.to_bytes()
        };
        if !ok {
            error(it, &et, "Unexpected layout of M-RAW header")?;
            return ret(n(1));
        }
    }
    // use same write directories as JPEG
    pl::method(it, &et, "InitWriteDirs", vec![pl::s("JPEG")])?;
    // rewrite the embedded JPEG in memory
    let jraf = new_raf(it, rf(&jpeg))?;
    let jpeg_info = hash(vec![("Parent", pl::s("RAF")), ("RAF", jraf), ("OutFile", rf(&out_jpeg))]);
    et.set("FILE_TYPE", pl::s("JPEG"));
    let success = pl::method(it, &et, "WriteJPEG", vec![jpeg_info])?;
    et.set("FILE_TYPE", pl::s("RAF"));
    if !(success.truthy() && val(&out_jpeg).truthy()) {
        error(it, &et, "Invalid RAF format")?;
        return ret(n(1));
    }
    if pl::lt(&success, &n(0)) {
        return ret(n(-1));
    }

    // rewrite the RAF image
    call(it, "SetByteOrder", vec![pl::s("MM")])?;
    let jpeg_len = pl::length(&val(&out_jpeg));
    // pad JPEG to an even 4 bytes (ALWAYS use padding as Fuji does)
    let pad_n = pl::sub(&n(4), &pl::modulo(&jpeg_len, &n(4))?);
    let pad = Value::bytes(vec![0u8; pad_n.to_i64().max(0) as usize]);
    // update JPEG size in header (size without padding)
    call(it, "Set32u", vec![pl::length(&val(&out_jpeg)), rf(&hdr), n(0x58)])?;
    // get pointer to start of the next RAF block
    let next_ptr = call(it, "Get32u", vec![rf(&hdr), n(0x5c)])?;
    // determine the length of padding at the end of the original JPEG
    let old_pad_len = pl::sub(&next_ptr, &pl::add(&jpos, &jlen));
    if old_pad_len.truthy() {
        let bad = pl::gt(&old_pad_len, &n(1000000))
            || pl::lt(&old_pad_len, &n(0))
            || !seek(it, &raf, pl::add(&jpos, &jlen), 0)?
            || !pl::num_eq(&read(it, &raf, &buff, old_pad_len.clone())?, &old_pad_len);
        if bad {
            error(it, &et, "Bad RAF pointer at 0x5c")?;
            return ret(n(1));
        }
        // make sure padding is only zero bytes (can be >100k for HS10)
        // (have seen non-null padding in X-Pro1)
        if re(r"[^\0]", "").is_match(&val(&buff)) && pl::method(it, &et, "Error", vec![pl::s("Non-null bytes found in padding"), n(2)])?.truthy() {
            return ret(n(1));
        }
    }
    // calculate offset difference due to change in JPEG size
    let ptr_diff = pl::sub(&pl::add(&pl::length(&val(&out_jpeg)), &pl::length(&pad)), &pl::add(&jlen, &old_pad_len));
    // update necessary pointers in header (0xcc and higher in M-RAW header)
    for offset in [0x5c, 0x64, 0x78, 0x80, 0xcc, 0x114, 0x164] {
        if pl::ge(&n(offset), &jpos) {
            break; // some versions have a short header
        }
        let old_ptr = call(it, "Get32u", vec![rf(&hdr), n(offset)])?;
        if !old_ptr.truthy() {
            continue; // don't update if pointer is zero
        }
        let mut new_ptr = pl::add(&old_ptr, &ptr_diff);
        if pl::lt(&new_ptr, &n(0)) || pl::gt(&new_ptr, &n(0xffffffff)) {
            if offset < 0xcc {
                error(it, &et, "Invalid offset in RAF header")?;
                return ret(n(1));
            }
            // assume values at 0xcc and greater are 8-byte integers (NC)
            // and adjust high word if necessary
            let mut high = call(it, "Get32u", vec![rf(&hdr), n(offset - 4)])?;
            if pl::lt(&new_ptr, &n(0)) {
                high = pl::sub(&high, &n(1));
                new_ptr = pl::add(&new_ptr, &pl::add(&n(0xffffffff), &n(1)));
                if pl::lt(&high, &n(0)) {
                    error(it, &et, "RAF header offset error")?;
                    return ret(n(1));
                }
            } else {
                high = pl::add(&high, &n(1));
                new_ptr = pl::sub(&new_ptr, &pl::add(&n(0xffffffff), &n(1)));
            }
            call(it, "Set32u", vec![high, rf(&hdr), n(offset - 4)])?;
        }
        call(it, "Set32u", vec![new_ptr, rf(&hdr), n(offset)])?;
    }
    // write the new header
    let outfile = dir_info.get("OutFile");
    let h = substr(it, &val(&hdr), &n(0), Some(&jpos))?;
    if !write(it, &outfile, vec![h])? {
        err = true;
    }
    // write the updated JPEG plus padding
    if !write(it, &outfile, vec![val(&out_jpeg), pad])? {
        err = true;
    }
    // copy over the rest of the RAF image
    if !seek(it, &raf, next_ptr, 0)? {
        error(it, &et, "Error reading RAF image")?;
        return ret(n(1));
    }
    while read(it, &raf, &buff, n(65536))?.truthy() {
        if !write(it, &outfile, vec![val(&buff)])? {
            err = true;
            break;
        }
    }
    ret(n(if err { -1 } else { 1 }))
}

//------------------------------------------------------------------------------
/// get information from FujiFilm RAW file (RAF)
/// Inputs: 0) ExifTool object reference, 1) dirInfo reference
/// Returns: 1 if this was a valid RAF file
pub fn process_raf(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info0) = (arg(&a, 0), arg(&a, 1));
    let (buff, jpeg) = (cell(Value::Undef), cell(Value::Undef));
    let mut warn = false;

    let raf = dir_info0.get("RAF");
    let base = or(dir_info0.get("Base"), || n(0));
    if !pl::num_eq(&read(it, &raf, &buff, n(0x70))?, &n(0x70)) {
        return ret(n(0));
    }
    if !re(r"^FUJIFILM", "").is_match(&val(&buff)) {
        return ret(n(0));
    }
    // get position and size of M-RAW header and jpeg preview
    let u = pl::unpack("x72NN", &val(&buff))?;
    let (_mpos, mlen) = (arg(&u, 0), arg(&u, 1));
    let u = pl::unpack("x84NN", &val(&buff))?;
    let (jpos, jlen) = (arg(&u, 0), arg(&u, 1));
    if ops::bit_and(&jpos, &n(0x8000)).truthy() {
        return ret(n(0));
    }
    if jpos.truthy() {
        if !seek(it, &raf, pl::add(&jpos, &base), 0)? {
            return ret(n(0));
        }
        if !pl::num_eq(&read(it, &raf, &jpeg, jlen.clone())?, &jlen) {
            return ret(n(0));
        }
    }
    call(it, "SetByteOrder", vec![pl::s("MM")])?;
    if !et.get("DOC_NUM").truthy() {
        pl::method(it, &et, "SetFileType", vec![])?;
    }
    let tbl = call(it, "GetTagTable", vec![pl::s("Image::ExifTool::FujiFilm::RAFHeader")])?;
    let d = hash(vec![("DataPt", rf(&buff)), ("DirName", pl::s("RAFHeader")), ("Base", base.clone())]);
    pl::method(it, &et, "ProcessDirectory", vec![d, tbl])?;

    // extract information from embedded JPEG
    let jraf = new_raf(it, rf(&jpeg))?;
    let dir_info = hash(vec![("Parent", pl::s("RAF")), ("RAF", jraf)]);
    if jpos.truthy() {
        let jb = pl::add(&jpos, &base);
        et.set("BASE", pl::add(&et.get("BASE"), &jb));
        let ok = pl::method(it, &et, "ProcessJPEG", vec![dir_info.clone()])?;
        et.set("BASE", pl::sub(&et.get("BASE"), &jb));
        if ok.truthy() {
            pl::method(it, &et, "FoundTag", vec![pl::s("PreviewImage"), rf(&jpeg)])?;
        }
    }
    // extract information from Fuji RAF and TIFF directories
    let (mut raf_num, mut ifd_num) = (pl::s(""), pl::s(""));
    for offset in [0x48i64, 0x5c, 0x64, 0x78, 0x80] {
        if jpos.truthy() && pl::ge(&n(offset), &jpos) {
            break;
        }
        if !(seek(it, &raf, pl::add(&n(offset), &base), 0)? && read(it, &raf, &buff, n(8))?.truthy()) {
            warn = true;
            break;
        }
        let u = pl::unpack("N2", &val(&buff))?;
        let (mut start, len) = (arg(&u, 0), arg(&u, 1));
        if !start.truthy() {
            continue;
        }
        start = pl::add(&start, &base);
        if offset == 0x64 || offset == 0x80 {
            // parse FujiIFD directory
            assign(&dir_info, vec![("RAF", raf.clone()), ("Base", start.clone())]);
            et.set("SET_GROUP1", pl::concat(&pl::s("FujiIFD"), &ifd_num));
            let tag_table_ptr = call(it, "GetTagTable", vec![pl::s("Image::ExifTool::FujiFilm::IFD")])?;
            // this is TIFF-format data only for some models, so no warning if it fails
            let proc_tiff = Value::Code(it.sub_ref("Image::ExifTool::ProcessTIFF"));
            if !pl::method(it, &et, "ProcessTIFF", vec![dir_info.clone(), tag_table_ptr, proc_tiff])?.truthy() {
                // do hash of image data if necessary
                if et.get("ImageDataHash").truthy() && seek(it, &raf, start.clone(), 0)? {
                    pl::method(it, &et, "ImageDataHash", vec![raf.clone(), len.clone(), pl::s("raw")])?;
                }
            }
            et.delete("SET_GROUP1");
            ifd_num = pl::add(&or(ifd_num, || n(1)), &n(1));
        } else if offset == 0x48 {
            let values = et.get("VALUE");
            values.set("FileType", pl::concat(&values.get("FileType"), &pl::s(" (M-RAW)")));
            if seek(it, &raf, start.clone(), 0)? && pl::num_eq(&read(it, &raf, &buff, mlen.clone())?, &mlen) {
                let tbl = call(it, "GetTagTable", vec![pl::s("Image::ExifTool::FujiFilm::MRAW")])?;
                let d = hash(vec![("DataPt", rf(&buff)), ("DataPos", start.clone()), ("DirName", pl::s("M-RAW"))]);
                pl::method(it, &et, "ProcessDirectory", vec![d, tbl])?;
            } else {
                pl::method(it, &et, "Warn", vec![pl::s("Error reading M-RAW header")])?;
            }
        } else {
            // parse RAF directory
            assign(&dir_info, vec![("RAF", raf.clone()), ("DirStart", start.clone())]);
            et.set("SET_GROUP1", pl::concat(&pl::s("RAF"), &raf_num));
            let tag_table_ptr = call(it, "GetTagTable", vec![pl::s("Image::ExifTool::FujiFilm::RAF")])?;
            if pl::method(it, &et, "ProcessDirectory", vec![dir_info.clone(), tag_table_ptr])?.truthy() {
                raf_num = pl::add(&or(raf_num, || n(1)), &n(1));
            } else {
                warn = true;
            }
            et.delete("SET_GROUP1");
        }
    }
    if warn {
        pl::method(it, &et, "Warn", vec![pl::s("Possibly corrupt RAF information")])?;
    }

    ret(n(1))
}

/// Register the subs ported so far; the others still run from the Perl.
pub fn register(it: &mut Interp) {
    it.define_native("Image::ExifTool::FujiFilm::ProcessFaceRec", process_face_rec);
    it.define_native("Image::ExifTool::FujiFilm::ProcessFujiDir", process_fuji_dir);
    it.define_native("Image::ExifTool::FujiFilm::ProcessMRAW", process_mraw);
    it.define_native("Image::ExifTool::FujiFilm::WriteRAF", write_raf);
    it.define_native("Image::ExifTool::FujiFilm::ProcessRAF", process_raf);
}
