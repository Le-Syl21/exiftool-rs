//! Image::ExifTool::Rawzor (Image/ExifTool/Rawzor.pm), ported sub by sub (see docs/port/PORTING.md).
//!
//! Read meta information from Rawzor compressed images. The file's
//! `my $implementedRawzorVersion` stays Perl data, read with `lex_scalar`.

use crate::interp::eval::ER;
use crate::interp::{Interp, Want};
use crate::modules::random_access::{cell, perl_eval, raf_read};
use crate::perl::Value;
use crate::pl::{self, arg, n, re, ret};
use crate::runtime;

const MODULE: &str = "Image::ExifTool::Rawzor";

fn s(x: &str) -> Value {
    pl::s(x)
}

fn call(it: &mut Interp, name: &str, args: Vec<Value>) -> ER<Value> {
    pl::call(it, &format!("Image::ExifTool::{name}"), args)
}

fn warn(it: &mut Interp, et: &Value, msg: Value) -> ER<()> {
    pl::method(it, et, "Warn", vec![msg])?;
    Ok(())
}

fn handle_tag(it: &mut Interp, et: &Value, table: &Value, tag: &str, val: Value) -> ER<()> {
    pl::method(it, et, "HandleTag", vec![table.clone(), s(tag), val])?;
    Ok(())
}

//------------------------------------------------------------------------------
// Extract information from a Rawzor file
// Inputs: 0) ExifTool object reference, 1) dirInfo reference
// Returns: 1 on success, 0 if this wasn't a valid Rawzor file
pub fn process_rwz(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info) = (arg(&a, 0), arg(&a, 1));
    let raf = dir_info.get("RAF");
    let mut buff = Value::Undef;

    // read the Rawzor file header:
    //  0 string - "rawzor" signature
    //  6 int16u - Required SDK version
    //  8 int16u - Creator SDK version
    // 10 int64u - RWZ file size
    // 18 int64u - original raw file size
    // 26 undef[12] - reserved
    // 38 int64u - metadata offset
    if !(pl::num_eq(&raf_read(it, &raf, &mut buff, n(46))?, &n(46)) && re(r"^rawzor", "").is_match(&buff)) {
        return ret(n(0));
    }

    call(it, "SetByteOrder", vec![s("II")])?;
    let bp = |b: &Value| pl::sref(b.clone());
    let req_vers = call(it, "Get16u", vec![bp(&buff), n(6)])?;
    let creator_vers = call(it, "Get16u", vec![bp(&buff), n(8)])?;
    let rwz_size = call(it, "Get64u", vec![bp(&buff), n(10)])?;
    let orig_size = call(it, "Get64u", vec![bp(&buff), n(18)])?;
    let tag_table_ptr = call(it, "GetTagTable", vec![s("Image::ExifTool::Rawzor::Main")])?;
    handle_tag(it, &et, &tag_table_ptr, "RawzorRequiredVersion", req_vers.clone())?;
    handle_tag(it, &et, &tag_table_ptr, "RawzorCreatorVersion", creator_vers)?;
    handle_tag(it, &et, &tag_table_ptr, "OriginalFileSize", orig_size.clone())?;
    if rwz_size.truthy() {
        handle_tag(it, &et, &tag_table_ptr, "CompressionFactor", pl::div(&orig_size, &rwz_size)?)?;
    }
    // check version numbers
    let implemented = runtime::lex_scalar(it, MODULE, "implementedRawzorVersion").borrow().clone();
    if pl::gt(&req_vers, &implemented) {
        warn(it, &et, pl::concat(&pl::concat(&s("Version "), &req_vers), &s(" Rawzor images not yet supported")))?;
        return ret(n(1));
    }
    let meta_offset = call(it, "Get64u", vec![bp(&buff), n(38)])?;
    if pl::gt(&meta_offset, &n(0x7fffffff)) {
        warn(it, &et, s("Bad metadata offset"))?;
        return ret(n(1));
    }
    // check for the ability to uncompress the information
    if pl::eval_block(it, |it| runtime::require(it, "IO::Uncompress::Bunzip2"))?.is_none() {
        warn(it, &et, s("Install IO::Compress::Bzip2 to decode Rawzor bzip2 compression"))?;
        return ret(n(1));
    }
    // read the metadata header:
    //  0 int64u - metadata section 0 end (offset in original file)
    //  8 int64u - metadata section 1 start
    // 16 int64u - metadata section 1 end
    // 24 int64u - metadata section 2 start
    // 32 undef[4] - reserved
    // 36 int32u - original metadata size
    // 40 int32u - compressed metadata size
    if !(pl::method(it, &raf, "Seek", vec![meta_offset, n(0)])?.truthy() && pl::num_eq(&raf_read(it, &raf, &mut buff, n(44))?, &n(44))) {
        warn(it, &et, s("Error reading metadata header"))?;
        return ret(n(1));
    }
    let meta_size = call(it, "Get32u", vec![bp(&buff), n(36)])?;
    if meta_size.truthy() {
        et.set("DontValidateImageData", n(1));
        // validate the metadata header and read the compressed metadata
        let end0 = call(it, "Get64u", vec![bp(&buff), n(0)])?;
        let pos1 = call(it, "Get64u", vec![bp(&buff), n(8)])?;
        let end1 = call(it, "Get64u", vec![bp(&buff), n(16)])?;
        let pos2 = call(it, "Get64u", vec![bp(&buff), n(24)])?;
        let len = call(it, "Get32u", vec![bp(&buff), n(40)])?;
        let ok = pl::num_eq(&raf_read(it, &raf, &mut buff, len.clone())?, &len)
            && pl::num_eq(&pl::add(&pl::add(&end0, &pl::sub(&end1, &pos1)), &pl::sub(&orig_size, &pos2)), &meta_size)
            && pl::le(&end0, &pos1)
            && pl::le(&pos1, &end1)
            && pl::le(&end1, &pos2);
        if !ok {
            warn(it, &et, s("Error reading image metadata"))?;
            return ret(n(1));
        }
        // uncompress the metadata
        let (b, b2) = (cell(buff.clone()), cell(Value::Undef));
        let r = perl_eval(it, r"IO::Uncompress::Bunzip2::bunzip2(\$buff, \$buf2)", MODULE, &[("buff", &b), ("buf2", &b2)], Want::Scalar)?.pop().unwrap_or_default();
        let buf2 = b2.borrow().clone();
        if !(r.truthy() && pl::length(&buf2).to_bytes() == meta_size.to_bytes()) {
            warn(it, &et, s("Error uncompressing image metadata"))?;
            return ret(n(1));
        }
        // re-assemble the original file (sans image data)
        let zeros = |k: Value| Value::bytes(vec![0u8; k.to_i64().max(0) as usize]);
        let parts = [
            pl::substr(&buf2, 0, Some(end0.to_i64())),
            zeros(pl::sub(&pos1, &end0)),
            pl::substr(&buf2, end0.to_i64(), Some(pl::sub(&end1, &pos1).to_i64())),
            zeros(pl::sub(&pos2, &end1)),
            pl::substr(&buf2, pl::sub(&pl::add(&end0, &end1), &pos1).to_i64(), Some(pl::sub(&orig_size, &pos2).to_i64())),
        ];
        let mut whole = Value::from("");
        for p in &parts {
            whole = pl::concat(&whole, p);
        }

        // extract original information by calling ExtractInfo recursively
        let opts = Value::new_hash();
        opts.set("ReEntry", n(1));
        pl::method(it, &et, "ExtractInfo", vec![pl::sref(whole), opts])?;
    }
    // set OriginalFileType from FileType of original file
    // then change FileType and MIMEType to indicate a Rawzor image
    let orig_file_type = et.get("FileType");
    if orig_file_type.truthy() {
        handle_tag(it, &et, &tag_table_ptr, "OriginalFileType", orig_file_type)?;
        pl::method(it, &et, "OverrideFileType", vec![s("RWZ")])?;
    } else {
        handle_tag(it, &et, &tag_table_ptr, "OriginalFileType", s("Unknown"))?;
        pl::method(it, &et, "SetFileType", vec![])?;
    }
    ret(n(1))
}

pub fn register(it: &mut Interp) {
    it.define_native("Image::ExifTool::Rawzor::ProcessRWZ", process_rwz);
}
