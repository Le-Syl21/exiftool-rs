//! Image::ExifTool::ZIP (Image/ExifTool/ZIP.pm), ported sub by sub (see docs/port/PORTING.md).
//!
//! Read ZIP archive meta information (and RAR, GZIP).
//!
//! The file's `my` hashes (%openDocType, %iWorkFile, %iWorkType) stay Perl
//! data, read with `lex_hash`.

use crate::interp::eval::ER;
use crate::interp::{Interp, Want};
use crate::modules::exiftool::part2::{get16u as get16u_n, get32u as get32u_n, get8u as get8u_n};
use crate::modules::random_access::{cell, perl_eval};
use crate::perl::regex as pre;
use crate::perl::{ops, Value};
use crate::pl::{self, arg, cap, n, re, ret};
use crate::runtime;

const MODULE: &str = "Image::ExifTool::ZIP";

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

/// `Image::ExifTool::NAME(ARGS)` (the functions imported with :DataAccess :Utils).
fn et_call(it: &mut Interp, name: &str, args: Vec<Value>) -> ER<Value> {
    pl::call(it, &format!("Image::ExifTool::{name}"), args)
}

fn get8u(it: &mut Interp, dp: &Value, pos: i64) -> ER<Value> {
    Ok(first(get8u_n(it, vec![dp.clone(), n(pos)], Want::Scalar)?))
}

fn get16u(it: &mut Interp, dp: &Value, pos: i64) -> ER<Value> {
    Ok(first(get16u_n(it, vec![dp.clone(), n(pos)], Want::Scalar)?))
}

fn get32u(it: &mut Interp, dp: &Value, pos: i64) -> ER<Value> {
    Ok(first(get32u_n(it, vec![dp.clone(), n(pos)], Want::Scalar)?))
}

/// `$raf->Read($buff, $len)`: the count, with `buff` updated.
fn read(it: &mut Interp, raf: &Value, buff: &mut Value, len: Value) -> ER<Value> {
    let mut args = [buff.clone(), len];
    let r = pl::method_mut(it, raf, "Read", &mut args, Want::Scalar)?;
    *buff = args[0].clone();
    Ok(first(r))
}

/// `$raf->Read($buff, $len) == $len`
fn read_eq(it: &mut Interp, raf: &Value, buff: &mut Value, len: &Value) -> ER<bool> {
    let r = read(it, raf, buff, len.clone())?;
    Ok(pl::num_eq(&r, len))
}

fn seek(it: &mut Interp, raf: &Value, off: Value, whence: i64) -> ER<bool> {
    Ok(pl::method(it, raf, "Seek", vec![off, n(whence)])?.truthy())
}

fn tell(it: &mut Interp, raf: &Value) -> ER<Value> {
    pl::method(it, raf, "Tell", vec![])
}

fn options(it: &mut Interp, et: &Value, name: &str) -> ER<Value> {
    pl::method(it, et, "Options", vec![s(name)])
}

fn warn(it: &mut Interp, et: &Value, msg: Value) -> ER<()> {
    pl::method(it, et, "Warn", vec![msg])?;
    Ok(())
}

fn vprint(it: &mut Interp, et: &Value, level: i64, msg: &str) -> ER<()> {
    pl::method(it, et, "VPrint", vec![n(level), s(msg)])?;
    Ok(())
}

fn handle_tag(it: &mut Interp, et: &Value, table: &Value, tag: Value, val: Value) -> ER<Value> {
    pl::method(it, et, "HandleTag", vec![table.clone(), tag, val])
}

fn found_tag(it: &mut Interp, et: &Value, tag: Value, val: Value) -> ER<Value> {
    pl::method(it, et, "FoundTag", vec![tag, val])
}

/// `ord($buff)`
fn ord(v: &Value) -> Value {
    n(v.to_bytes().first().map_or(0, |b| *b as i64))
}

/// `my ($a, $b) = $zip->contents($member)`
fn contents(it: &mut Interp, zip: &Value, member: &Value) -> ER<(Value, Value)> {
    let r = pl::method_list(it, zip, "contents", vec![member.clone()])?;
    Ok((pl::arg(&r, 0), pl::arg(&r, 1)))
}

/// `$$h{k} = v` on the ExifTool object (`$$et{DOC_NUM} = ++$docNum`).
fn set_doc_num(et: &Value, doc_num: i64) {
    et.set("DOC_NUM", n(doc_num));
}

pub fn warn_proc(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let v = arg(&a, 0);
    it.set_scalar("Image::ExifTool::ZIP::warnString", v.clone());
    ret(v)
}

//------------------------------------------------------------------------------
// Read unsigned LEB (Little Endian Base) from file
// Inputs: 0) RAF ref
// Returns: integer value
fn read_uleb_raf(it: &mut Interp, raf: &Value) -> ER<Value> {
    let mut buff = Value::Undef;
    let mut rtn_val = n(0);
    let mut i: i64 = 0;
    loop {
        if !read(it, raf, &mut buff, n(1))?.truthy() {
            break;
        }
        let num = ord(&buff);
        rtn_val = pl::add(&rtn_val, &ops::shift_left(&ops::bit_and(&num, &n(0x7f)), &n(i * 7)));
        if ops::bit_and(&num, &n(0x80)).to_i64() == 0 {
            break;
        }
        i += 1;
    }
    Ok(rtn_val)
}

pub fn read_uleb(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    ret(read_uleb_raf(it, &arg(&a, 0))?)
}

//------------------------------------------------------------------------------
// Extract information from a RAR file
// Inputs: 0) ExifTool object reference, 1) dirInfo reference
// Returns: 1 on success, 0 if this wasn't a valid RAR file
pub fn process_rar(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info) = (arg(&a, 0), arg(&a, 1));
    let raf = dir_info.get("RAF");
    let mut buff = Value::Undef;
    let mut doc_num: i64 = 0;

    if !(read(it, &raf, &mut buff, n(7))?.truthy() && re(r"Rar!\x1a\x07[\0\x01]", "").is_match(&buff)) {
        return ret(n(0));
    }

    if pl::str_eq(&buff, "Rar!\x1a\x07\0") {
        // RARv4 (ref 4)

        pl::method(it, &et, "SetFileType", vec![])?;
        et_call(it, "SetByteOrder", vec![s("II")])?;
        let mut tag_table_ptr = et_call(it, "GetTagTable", vec![s("Image::ExifTool::ZIP::RAR5")])?;
        handle_tag(it, &et, &tag_table_ptr, s("FileVersion"), s("RAR v4"))?;
        tag_table_ptr = et_call(it, "GetTagTable", vec![s("Image::ExifTool::ZIP::RAR")])?;

        loop {
            // read block header
            if !read_eq(it, &raf, &mut buff, &n(7))? {
                break;
            }
            let u = pl::unpack("xxCvv", &buff)?;
            let (type_, flags, mut size) = (arg(&u, 0), arg(&u, 1), arg(&u, 2));
            size = pl::sub(&size, &n(7));
            if ops::bit_and(&flags, &n(0x8000)).to_i64() != 0 {
                if !read_eq(it, &raf, &mut buff, &n(4))? {
                    break;
                }
                size = pl::add(&size, &pl::sub(&pl::unpack1("V", &buff)?, &n(4)));
            }
            if pl::lt(&size, &n(0)) {
                break;
            }
            if !size.truthy() {
                continue; // ignore blocks with no data
            }
            // don't try to read very large blocks unless LargeFileSupport is enabled
            if pl::ge(&size, &n(0x80000000)) {
                if !options(it, &et, "LargeFileSupport")?.truthy() {
                    warn(it, &et, s("Large block encountered. Aborting."))?;
                    break;
                } else if pl::str_eq(&options(it, &et, "LargeFileSupport")?, "2") {
                    warn(it, &et, s("Processing large block (LargeFileSupport is 2)"))?;
                }
            }
            // process the block
            if pl::num_eq(&type_, &n(0x74)) {
                // file block
                // read maximum 4 KB from a file block
                let nn = if pl::gt(&size, &n(4096)) { n(4096) } else { size.clone() };
                if !read_eq(it, &raf, &mut buff, &nn)? {
                    break;
                }
                // add compressed size to start of data so we can extract it with the other tags
                buff = pl::concat(&pl::pack("V", &[size.clone()])?, &buff);
                doc_num += 1;
                set_doc_num(&et, doc_num);
                let di = Value::new_hash();
                di.set("DataPt", pl::sref(buff.clone()));
                pl::method(it, &et, "ProcessDirectory", vec![di, tag_table_ptr.clone()])?;
                size = pl::sub(&size, &nn);
            } else if pl::num_eq(&type_, &n(0x75)) && pl::gt(&size, &n(6)) {
                // comment block
                if !read_eq(it, &raf, &mut buff, &size)? {
                    break;
                }
                // save comment, only if "Stored" (this is untested)
                if pl::num_eq(&get8u(it, &pl::sref(buff.clone()), 3)?, &n(0x30)) {
                    found_tag(it, &et, s("Comment"), pl::substr(&buff, 6, None))?;
                }
                continue;
            }
            // seek to the start of the next block
            if size.truthy() && !seek(it, &raf, size.clone(), 1)? {
                break;
            }
        }
    } else {
        // RARv5 (ref 7, github#203)

        if !(read(it, &raf, &mut buff, n(1))?.truthy() && pl::str_eq(&buff, "\0")) {
            return ret(n(0));
        }
        pl::method(it, &et, "SetFileType", vec![])?;
        let tag_table_ptr = et_call(it, "GetTagTable", vec![s("Image::ExifTool::ZIP::RAR5")])?;
        handle_tag(it, &et, &tag_table_ptr, s("FileVersion"), s("RAR v5"))?;
        et.set("INDENT", pl::concat(&et.get("INDENT"), &s("| ")));

        // loop through header blocks
        loop {
            seek(it, &raf, n(4), 1)?; // skip header CRC
            let head_size = read_uleb_raf(it, &raf)?;
            if pl::num_eq(&head_size, &n(0)) {
                break;
            }
            // read the header and create new RAF object for reading it
            let mut header = Value::Undef;
            if !read_eq(it, &raf, &mut header, &head_size)? {
                break;
            }
            let raf_hdr = pl::method(it, &s("File::RandomAccess"), "new", vec![pl::sref(header.clone())])?;
            let head_type = read_uleb_raf(it, &raf_hdr)?; // get header type

            if pl::num_eq(&head_type, &n(4)) {
                // encryption block
                pl::method(it, &et, "Warn", vec![s("File is encrypted."), n(0)])?;
                break;
            }
            // skip over all headers except file or service header
            if !(pl::num_eq(&head_type, &n(2)) || pl::num_eq(&head_type, &n(3))) {
                continue;
            }
            if pl::num_eq(&head_type, &n(2)) {
                pl::method(it, &et, "VerboseDir", vec![s("RAR5 file"), Value::Undef, head_size.clone()])?;
            }

            let head_flag = read_uleb_raf(it, &raf_hdr)?;
            read_uleb_raf(it, &raf_hdr)?; // skip extraSize
            let data_size;
            if ops::bit_and(&head_flag, &n(0x0002)).to_i64() != 0 {
                data_size = read_uleb_raf(it, &raf_hdr)?; // compressed data size
                if pl::num_eq(&head_type, &n(2)) {
                    handle_tag(it, &et, &tag_table_ptr, s("CompressedSize"), data_size.clone())?;
                } else {
                    seek(it, &raf, data_size.clone(), 1)?; // skip service data section
                    continue;
                }
            } else {
                if pl::num_eq(&head_type, &n(3)) {
                    continue; // all done with service header
                }
                data_size = n(0);
            }
            let file_flag = read_uleb_raf(it, &raf_hdr)?;
            let uncompressed_size = read_uleb_raf(it, &raf_hdr)?;
            if ops::bit_and(&file_flag, &n(0x0008)).to_i64() == 0 {
                handle_tag(it, &et, &tag_table_ptr, s("UncompressedSize"), uncompressed_size)?;
            }
            read_uleb_raf(it, &raf_hdr)?; // skip file attributes
            if ops::bit_and(&file_flag, &n(0x0002)).to_i64() != 0 {
                if !read_eq(it, &raf_hdr, &mut buff, &n(4))? {
                    break;
                }
                // (untested)
                let u = pl::unpack("V", &buff)?;
                let mut args = vec![tag_table_ptr.clone(), s("ModifyDate")];
                args.extend(u);
                pl::method(it, &et, "HandleTag", args)?;
            }
            if ops::bit_and(&file_flag, &n(0x0004)).to_i64() != 0 {
                seek(it, &raf_hdr, n(4), 1)?; // skip CRC if present
            }

            read_uleb_raf(it, &raf_hdr)?; // skip compressionInfo

            // get operating system
            let os = read_uleb_raf(it, &raf_hdr)?;
            handle_tag(it, &et, &tag_table_ptr, s("OperatingSystem"), os)?;

            // get filename
            if !read_eq(it, &raf_hdr, &mut buff, &n(1))? {
                break;
            }
            let name_len = ord(&buff);
            if !read_eq(it, &raf_hdr, &mut buff, &name_len)? {
                break;
            }
            re(r"\0+$", "").subst(&mut buff, false, |_| Vec::new()); // remove trailing nulls (if any)
            handle_tag(it, &et, &tag_table_ptr, s("ArchivedFileName"), buff.clone())?;

            doc_num += 1;
            set_doc_num(&et, doc_num);

            seek(it, &raf, data_size, 1)?; // skip data section
        }
        let indent = et.get("INDENT");
        et.set("INDENT", pl::substr(&indent, 0, Some(-2)));
    }

    et.set("DOC_NUM", n(0));
    if doc_num > 1 && !options(it, &et, "Duplicates")?.truthy() {
        pl::method(it, &et, "Warn", vec![Value::from(format!("Use the Duplicates option to extract tags for all {doc_num} files")), n(1)])?;
    }

    ret(n(1))
}

/// A scalar `$str =~ /PAT/g` from pos() `p`: the end of the match (and
/// `p` set to it); on failure `p` becomes None, as perl resets pos().
fn match_g_end(pat: &str, subject: &[u8], p: &mut Option<usize>) -> Option<usize> {
    let rx = pre::compile(pat.as_bytes(), "").unwrap_or_else(|e| panic!("bad pattern /{pat}/: {e}"));
    let start = p.unwrap_or(0);
    let r = if start > subject.len() { Ok(None) } else { rx.match_at(subject, start) };
    match r {
        Ok(Some(sp)) => {
            let e = sp[0].unwrap().1;
            *p = Some(e);
            Some(e)
        }
        _ => {
            *p = None;
            None
        }
    }
}

//------------------------------------------------------------------------------
// Extract information from a GNU ZIP file (ref 3)
// Inputs: 0) ExifTool object reference, 1) dirInfo reference
// Returns: 1 on success, 0 if this wasn't a valid GZIP file
pub fn process_gzip(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info) = (arg(&a, 0), arg(&a, 1));
    let raf = dir_info.get("RAF");
    let mut buff = Value::Undef;

    if !(read(it, &raf, &mut buff, n(10))?.truthy() && re(r"^\x1f\x8b\x08", "").is_match(&buff)) {
        return ret(n(0));
    }

    pl::method(it, &et, "SetFileType", vec![])?;
    et_call(it, "SetByteOrder", vec![s("II")])?;

    let tag_table_ptr = et_call(it, "GetTagTable", vec![s("Image::ExifTool::ZIP::GZIP")])?;
    let bp = pl::sref(buff.clone());
    let v = get8u(it, &bp, 2)?;
    handle_tag(it, &et, &tag_table_ptr, n(2), v)?;
    let flags = get8u(it, &bp, 3)?;
    handle_tag(it, &et, &tag_table_ptr, n(3), flags.clone())?;
    let v = get32u(it, &bp, 4)?;
    handle_tag(it, &et, &tag_table_ptr, n(4), v)?;
    let v = get8u(it, &bp, 8)?;
    handle_tag(it, &et, &tag_table_ptr, n(8), v)?;
    let v = get8u(it, &bp, 9)?;
    handle_tag(it, &et, &tag_table_ptr, n(9), v)?;

    // extract file name and comment if they exist
    let fl = flags.to_i64();
    if fl & 0x18 != 0 {
        if fl & 0x04 != 0 {
            // skip extra field
            if !read_eq(it, &raf, &mut buff, &n(2))? {
                return ret(n(1));
            }
            let len = get16u(it, &pl::sref(buff.clone()), 0)?;
            if !read_eq(it, &raf, &mut buff, &len)? {
                return ret(n(1));
            }
        }
        if !read(it, &raf, &mut buff, n(4096))?.truthy() {
            return ret(n(1));
        }
        let data = buff.to_bytes();
        let mut p: Option<usize> = None; // pos($buff)
        let mut pos: i64 = 0;
        // loop for ArchivedFileName (10) and Comment (11) tags
        for tag_id in [10i64, 11] {
            let mask = if tag_id == 10 { 0x08 } else { 0x10 };
            if fl & mask == 0 {
                continue;
            }
            let end = match match_g_end(r"\0", &data, &mut p) {
                Some(e) => e as i64 - 1,
                None => data.len() as i64,
            };
            // (the doc specifies the string should be ISO 8859-1,
            // but in OS X it seems to be UTF-8, so don't translate
            // it because I could just as easily screw it up)
            let str_ = pl::substr(&buff, pos, Some(end - pos));
            handle_tag(it, &et, &tag_table_ptr, n(tag_id), str_)?;
            if end >= data.len() as i64 {
                break;
            }
            pos = end + 1;
        }
    }
    ret(n(1))
}

//------------------------------------------------------------------------------
// Call HandleTags for attributes of an Archive::Zip member
// Inputs: 0) ExifTool object ref, 1) member ref, 2) optional tag table ref
fn handle_member_rs(it: &mut Interp, et: &Value, member: &Value, tag_table_ptr: &Value) -> ER<Value> {
    let mut tag_table_ptr = tag_table_ptr.clone();
    if !tag_table_ptr.truthy() {
        tag_table_ptr = et_call(it, "GetTagTable", vec![s("Image::ExifTool::ZIP::Main")])?;
    }
    for (id, m) in [(2, "versionNeededToExtract"), (3, "bitFlag"), (4, "compressionMethod"), (5, "lastModFileDateTime"), (7, "crc32"), (9, "compressedSize"), (11, "uncompressedSize"), (15, "fileName")] {
        let v = pl::method(it, member, m, vec![])?;
        handle_tag(it, et, &tag_table_ptr, n(id), v)?;
    }
    let com = pl::method(it, member, "fileComment", vec![])?;
    if !com.is_undef() && pl::length(&com).to_i64() != 0 {
        return handle_tag(it, et, &tag_table_ptr, s("_com"), com);
    }
    Ok(Value::Undef)
}

pub fn handle_member(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let r = handle_member_rs(it, &arg(&a, 0), &arg(&a, 1), &arg(&a, 2))?;
    ret(r)
}

//------------------------------------------------------------------------------
// Extract file from ZIP archive
// Inputs: 0) ExifTool ref, 1) Zip object ref, 2) file name
// Returns: zip member or undef it it didn't exist
fn extract_file_rs(it: &mut Interp, et: &Value, zip: &Value, file: &Value) -> ER<Value> {
    let result = pl::method(it, zip, "memberNamed", vec![file.clone()])?;
    let msg = cat(&[s("  (Extracting '"), file.clone(), s("' from zip archive)\n")]);
    pl::method(it, et, "VPrint", vec![n(1), msg])?;
    Ok(result)
}

pub fn extract_file(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let r = extract_file_rs(it, &arg(&a, 0), &arg(&a, 1), &arg(&a, 2))?;
    ret(r)
}

/// `eval { require MODULE }`: true if it loaded.
fn eval_require(it: &mut Interp, m: &str) -> ER<bool> {
    Ok(pl::eval_block(it, |it| runtime::require(it, m))?.is_some())
}

/// `my %dirInfo = (...)` with an `$$et{NO_STRUCT_WARN}` save/set/restore
/// around ProcessDirectory.
fn process_no_struct_warn(it: &mut Interp, et: &Value, dir_info: Value, table: &str) -> ER<()> {
    // (avoid structure warnings when copying from XML)
    let old_warn = et.get("NO_STRUCT_WARN");
    et.set("NO_STRUCT_WARN", n(1));
    let t = et_call(it, "GetTagTable", vec![s(table)])?;
    pl::method(it, et, "ProcessDirectory", vec![dir_info, t])?;
    et.set("NO_STRUCT_WARN", old_warn);
    Ok(())
}

/// The part of ProcessZIP's `for (;;)` loop body that runs with
/// `local $SIG{'__WARN__'} = \&WarnProc` (from that statement to the end
/// of the body; every path leaves the loop with `last`).
fn zip_body(it: &mut Interp, et: &Value, dir_info: &Value, raf: &Value, fh: &mut Value, zip: &mut Value, doc_num: &mut i64, tag_table_ptr: &Value) -> ER<()> {
    let mut buff;
    let mut status = pl::method(it, zip, "readFromFileHandle", vec![fh.clone()])?;
    if pl::str_eq(&status, "4") && pl::ge(&raf.get("TESTED"), &n(0)) && eval_require(it, "IO::String")? && seek(it, raf, n(0), 2)? && pl::lt(&tell(it, raf)?, &n(100000000)) {
        // try again, reading it ourself this time in an attempt to avoid
        // a failed test with Perl 5.6.2 GNU/Linux 2.6.32-5-686 i686-linux-64int-ld
        seek(it, raf, n(0), 0)?;
        pl::method(it, raf, "Slurp", vec![])?;
        *fh = pl::method(it, &s("IO::String"), "new", vec![raf.get("BUFF_PT").deref()])?;
        *zip = pl::method(it, &s("Archive::Zip"), "new", vec![])?;
        status = pl::method(it, zip, "readFromFileHandle", vec![fh.clone()])?;
    }
    if status.truthy() {
        *zip = Value::Undef;
        let err = match &*status.to_bytes() {
            b"1" => s("Stream end error"),
            b"3" => s("Format error"),
            b"4" => s("IO error"),
            _ => cat(&[s("Error "), status.clone()]),
        };
        warn(it, et, cat(&[err, s(" reading ZIP file")]))?;
        return Ok(());
    }
    // extract zip file comment
    let comment = pl::method(it, zip, "zipfileComment", vec![])?;
    if !comment.is_undef() && pl::length(&comment).to_i64() != 0 {
        found_tag(it, et, s("Comment"), comment)?;
    }

    dir_info.set("ZIP", zip.clone());

    // check for an Office Open file (DOCX, etc)
    // --> read '[Content_Types].xml' to determine the file type
    let mut mime = Value::Undef;
    let mut members: Vec<Value> = Vec::new();
    let c_type = extract_file_rs(it, et, zip, &s("[Content_Types].xml"))?;
    if c_type.truthy() {
        let (b, st) = contents(it, zip, &c_type)?;
        buff = b;
        status = st;
        if !status.truthy() {
            // first look for the main document with the expected name
            let m = re(r#"\sPartName\s*=\s*['"](?:/ppt/presentation.xml|/word/document.xml|/xl/workbook.xml)['"][^>]*\sContentType\s*=\s*(['"])([^"']+)\.main(\+xml)?\1"#, "")
                .m(&buff)
                // then look for the main part
                .or_else(|| re(r#"<Override[^>]*\sPartName[^<]+\sContentType\s*=\s*(['"])([^"']+)\.main(\+xml)?\1"#, "").m(&buff))
                // and if all else fails, use the default main
                .or_else(|| re(r#"ContentType\s*=\s*(['"])([^"']+)\.main(\+xml)?\1"#, "").m(&buff));
            if let Some(m) = m {
                mime = cap(&m, 2);
            }
        }
    }
    // check for docProps if we couldn't find a MIME type
    if !mime.truthy() {
        members = pl::method_list(it, zip, "membersMatching", vec![s(r"^docProps/.*\.(xml|XML)$")])?;
    }
    if mime.truthy() || !members.is_empty() {
        dir_info.set("MIME", mime.clone());
        runtime::require(it, "Image::ExifTool::OOXML")?;
        pl::call(it, "Image::ExifTool::OOXML::ProcessDOCX", vec![et.clone(), dir_info.clone()])?;
        dir_info.delete("MIME");
        return Ok(());
    }

    // check for an EIP file
    members = pl::method_list(it, zip, "membersMatching", vec![s(r"^CaptureOne/.*\.(cos|COS)$")])?;
    if !members.is_empty() {
        runtime::require(it, "Image::ExifTool::CaptureOne")?;
        pl::call(it, "Image::ExifTool::CaptureOne::ProcessEIP", vec![et.clone(), dir_info.clone()])?;
        return Ok(());
    }

    // check for an iWork file
    members = pl::method_list(it, zip, "membersMatching", vec![s(r"(?i)^(index\.(xml|apxl)|QuickLook/Thumbnail\.jpg|[^/]+\.(pages|numbers|key)/Index.(zip|xml|apxl))$")])?;
    if !members.is_empty() {
        runtime::require(it, "Image::ExifTool::iWork")?;
        pl::call(it, "Image::ExifTool::iWork::Process_iWork", vec![et.clone(), dir_info.clone()])?;
        return Ok(());
    }

    // check for an Open Document, IDML or EPUB file
    let open_doc_type = runtime::lex_hash(it, MODULE, "openDocType");
    let m_type = extract_file_rs(it, et, zip, &s("mimetype"))?;
    if m_type.truthy() {
        let (mm, st) = contents(it, zip, &m_type)?;
        mime = mm;
        status = st;
        let m1 = if status.truthy() { None } else { re(r"([\x21-\xfe]+)", "s").m(&mime) };
        if let Some(m1) = m1 {
            // clean up MIME type just in case (note that MIME is case insensitive)
            mime = pl::lc(&cap(&m1, 1));
            let odt = open_doc_type.get(mime.to_bytes());
            let ft = if odt.truthy() { odt.clone() } else { s("ZIP") };
            pl::method(it, et, "SetFileType", vec![ft, mime.clone()])?;
            if !odt.truthy() {
                warn(it, et, cat(&[s("Unrecognized MIMEType "), mime.clone()]))?;
            }
            // extract Open Document metadata from "meta.xml"
            let mut meta = extract_file_rs(it, et, zip, &s("meta.xml"))?;
            // IDML files have metadata in a different place (ref 6)
            if !meta.truthy() {
                meta = extract_file_rs(it, et, zip, &s("META-INF/metadata.xml"))?;
            }
            if meta.truthy() {
                let (b, st) = contents(it, zip, &meta)?;
                buff = b;
                status = st;
                if !status.truthy() {
                    let di = Value::new_hash();
                    di.set("DirName", s("XML"));
                    di.set("DataPt", pl::sref(buff.clone()));
                    di.set("DirLen", pl::length(&buff));
                    di.set("DataLen", pl::length(&buff));
                    process_no_struct_warn(it, et, di, "Image::ExifTool::XMP::Main")?;
                }
            }
            // process rootfile of EPUB container if applicable
            #[allow(clippy::never_loop)]
            loop {
                if meta.truthy() && !pl::str_eq(&mime, "application/epub+zip") {
                    break;
                }
                let container = extract_file_rs(it, et, zip, &s("META-INF/container.xml"))?;
                let (b, st) = contents(it, zip, &container)?;
                buff = b;
                status = st;
                if status.truthy() {
                    break;
                }
                let Some(rm) = re(r#"<rootfile\s+[^>]*?\bfull-path=(['"])(.*?)\1"#, "s").m(&buff) else { break };
                // load the rootfile data (OPF extension; contains XML metadata)
                let meta2 = pl::method(it, zip, "memberNamed", vec![cap(&rm, 2)])?;
                if !meta2.truthy() {
                    break;
                }
                meta = meta2;
                let (b, st) = contents(it, zip, &meta)?;
                buff = b;
                status = st;
                if status.truthy() {
                    break;
                }
                // use opf:event to generate more meaningful tag names for dc:date
                loop {
                    let mut ev = Value::Undef;
                    let cnt = re(r#"<dc:date opf:event="(\w+)">([^<]+)</dc:date>"#, "s").subst(&mut buff, false, |m| {
                        ev = cap(m, 1);
                        cat(&[s("<dc:"), cap(m, 1), s("Date>"), cap(m, 2), s("</dc:"), cap(m, 1), s("Date>")]).to_bytes().to_vec()
                    });
                    if cnt == 0 {
                        break;
                    }
                    let dc_table = et_call(it, "GetTagTable", vec![s("Image::ExifTool::XMP::dc")])?;
                    let tag = cat(&[ev, s("Date")]);
                    if !dc_table.get(tag.to_bytes()).truthy() {
                        let info = Value::new_hash();
                        info.set("Name", pl::ucfirst(&tag));
                        let groups = Value::new_hash();
                        groups.set("2", s("Time"));
                        info.set("Groups", groups);
                        info.set("List", s("Seq"));
                        let dti = pl::pkg_hash(it, "Image::ExifTool::XMP::dateTimeInfo");
                        for k in dti.keys() {
                            info.set(&k[..], dti.get(&k[..]));
                        }
                        et_call(it, "AddTagToTable", vec![dc_table, tag, info])?;
                    }
                }
                let di = Value::new_hash();
                di.set("DataPt", pl::sref(buff.clone()));
                di.set("DirLen", pl::length(&buff));
                di.set("DataLen", pl::length(&buff));
                let ip = Value::new_hash();
                ip.set("package", n(1));
                ip.set("metadata", n(1));
                di.set("IgnoreProp", ip);
                process_no_struct_warn(it, et, di, "Image::ExifTool::XMP::XML")?;
                break;
            }
            if open_doc_type.get(mime.to_bytes()).truthy() || meta.truthy() {
                // extract preview image(s) from "Thumbnails" directory if they exist
                for (type_, tag) in [("jpg", "PreviewImage"), ("png", "PreviewPNG")] {
                    let thumb = extract_file_rs(it, et, zip, &Value::from(format!("Thumbnails/thumbnail.{type_}")))?;
                    if !thumb.truthy() {
                        continue;
                    }
                    let (b, st) = contents(it, zip, &thumb)?;
                    buff = b;
                    status = st;
                    if !status.truthy() {
                        found_tag(it, et, s(tag), buff.clone())?;
                    }
                }
                return Ok(()); // all done since we recognized the MIME type or found metadata
            }
            // continue on to list ZIP contents...
        }
    }

    // otherwise just extract general ZIP information
    pl::method(it, et, "SetFileType", vec![])?;
    members = pl::method_list(it, zip, "members", vec![])?;
    let mut i_work_type = Value::Undef;
    let i_work_type_h = runtime::lex_hash(it, MODULE, "iWorkType");
    let i_work_file = runtime::lex_hash(it, MODULE, "iWorkFile");
    // special files to extract
    let extract = |file: &[u8]| -> Option<&'static str> {
        Some(match file {
            b"meta.json" => "1",
            b"previews/preview.png" => "PreviewPNG",
            b"preview.jpg" => "PreviewImage",           // (iWork 2013 files)
            b"preview-web.jpg" => "OtherImage",         // (iWork 2013 files)
            b"preview-micro.jpg" => "ThumbnailImage",   // (iWork 2013 files)
            b"QuickLook/Thumbnail.jpg" => "ThumbnailImage", // (iWork 2009 files)
            b"QuickLook/Preview.pdf" => "PreviewPDF",   // (iWork 2009 files)
            _ => return None,
        })
    };
    for member in members {
        *doc_num += 1;
        set_doc_num(et, *doc_num);
        handle_member_rs(it, et, &member, tag_table_ptr)?;
        let file = pl::method(it, &member, "fileName", vec![])?;
        let fb = file.to_bytes();
        // extract things from Sketch files
        if let Some(ex) = extract(&fb) {
            let (b, st) = contents(it, zip, &member)?;
            buff = b;
            status = st;
            if status.truthy() {
                warn(it, et, cat(&[s("Error extracting "), file.clone()]))?;
                continue;
            }
            if pl::str_eq(&file, "meta.json") {
                let opts = Value::new_hash();
                opts.set("ReEntry", n(1));
                pl::method(it, et, "ExtractInfo", vec![pl::sref(buff.clone()), opts])?;
                let app = et.get("VALUE").get("App");
                if app.truthy() && re(r"sketch", "i").is_match(&app) {
                    pl::method(it, et, "OverrideFileType", vec![s("SKETCH")])?;
                }
            } else {
                found_tag(it, et, s(ex), buff.clone())?;
            }
        } else if pl::str_eq(&file, "Index/Document.iwa") && !i_work_type.truthy() {
            let ext = et.get("FILE_EXT");
            let key = if ext.truthy() { ext } else { s("") };
            let type_ = i_work_type_h.get(key.to_bytes());
            i_work_type = if type_.truthy() { type_ } else { s("PAGES") };
        } else if i_work_file.get(&fb[..]).truthy() {
            i_work_type = i_work_file.get(&fb[..]);
        }
    }
    if i_work_type.truthy() {
        pl::method(it, et, "OverrideFileType", vec![i_work_type])?;
    }
    Ok(())
}

//------------------------------------------------------------------------------
// Extract information from a ZIP file
// Inputs: 0) ExifTool object reference, 1) dirInfo reference
// Returns: 1 on success, 0 if this wasn't a valid ZIP file
pub fn process_zip(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info) = (arg(&a, 0), arg(&a, 1));
    let raf = dir_info.get("RAF");
    let (mut buff, mut buf2, mut zip) = (Value::Undef, Value::Undef, Value::Undef);

    if !(read_eq(it, &raf, &mut buff, &n(30))? && re(r"^PK\x03\x04", "").is_match(&buff)) {
        return ret(n(0));
    }

    let tag_table_ptr = et_call(it, "GetTagTable", vec![s("Image::ExifTool::ZIP::Main")])?;
    let mut doc_num: i64 = 0;

    // use Archive::Zip if available
    #[allow(clippy::never_loop)]
    loop {
        if !(eval_require(it, "Archive::Zip")? && eval_require(it, "IO::File")?) {
            let ext = et.get("FILE_EXT");
            if ext.truthy() && !pl::str_eq(&ext, "ZIP") {
                warn(it, &et, s("Install Archive::Zip to decode compressed ZIP information"))?;
            }
            break;
        }
        // Archive::Zip requires a seekable IO::File object
        let mut fh;
        if pl::ge(&raf.get("TESTED"), &n(0)) {
            if !eval_require(it, "IO::File")? {
                // (this shouldn't happen because IO::File is a prerequisite of Archive::Zip)
                warn(it, &et, s("Install IO::File to decode compressed ZIP information"))?;
                break;
            }
            seek(it, &raf, n(0), 0)?;
            fh = raf.get("FILE_PT");
            let c = cell(fh.clone());
            perl_eval(it, "bless $fh, 'IO::File'", MODULE, &[("fh", &c)], Want::Void)?; // Archive::Zip expects an IO::File object
            fh = c.borrow().clone();
        } else if eval_require(it, "IO::String")? {
            // read the whole file into memory (what else can I do?)
            pl::method(it, &raf, "Slurp", vec![])?;
            fh = pl::method(it, &s("IO::String"), "new", vec![raf.get("BUFF_PT").deref()])?;
        } else {
            let type_ = if raf.get("FILE_PT").truthy() { "pipe or socket" } else { "scalar reference" };
            warn(it, &et, Value::from(format!("Install IO::String to decode compressed ZIP information from a {type_}")))?;
            break;
        }
        vprint(it, &et, 1, "  --- using Archive::Zip ---\n")?;
        zip = pl::method(it, &s("Archive::Zip"), "new", vec![])?;
        // catch all warnings! (Archive::Zip is bad for this)
        let handler = Value::Code(it.sub_ref("Image::ExifTool::ZIP::WarnProc"));
        pl::local_sig_warn(it, handler, |it| zip_body(it, &et, &dir_info, &raf, &mut fh, &mut zip, &mut doc_num, &tag_table_ptr))?;
        break;
    }
    // all done if we processed this using Archive::Zip
    if zip.truthy() {
        dir_info.delete("ZIP");
        et.delete("DOC_NUM");
        if doc_num > 1 && !options(it, &et, "Duplicates")?.truthy() {
            pl::method(it, &et, "Warn", vec![Value::from(format!("Use the Duplicates option to extract tags for all {doc_num} files")), n(1)])?;
        }
        return ret(n(1));
    }
    //
    // process the ZIP file by hand (funny, but this seems easier than using Archive::Zip)
    //
    vprint(it, &et, 1, "  -- processing as binary data --\n")?;
    seek(it, &raf, n(30), 0)?;
    pl::method(it, &et, "SetFileType", vec![])?;
    et_call(it, "SetByteOrder", vec![s("II")])?;

    //  A.  Local file header:
    //  local file header signature     0) 4 bytes  (0x04034b50)
    //  version needed to extract       4) 2 bytes
    //  general purpose bit flag        6) 2 bytes
    //  compression method              8) 2 bytes
    //  last mod file time             10) 2 bytes
    //  last mod file date             12) 2 bytes
    //  crc-32                         14) 4 bytes
    //  compressed size                18) 4 bytes
    //  uncompressed size              22) 4 bytes
    //  file name length               26) 2 bytes
    //  extra field length             28) 2 bytes
    loop {
        let bp = pl::sref(buff.clone());
        let mut len = pl::add(&get16u(it, &bp, 26)?, &get16u(it, &bp, 28)?);
        if !read_eq(it, &raf, &mut buf2, &len)? {
            break;
        }

        doc_num += 1;
        set_doc_num(&et, doc_num);
        buff = pl::concat(&buff, &buf2);
        let data_pt = pl::sref(buff.clone());
        let di = Value::new_hash();
        di.set("DataPt", data_pt.clone());
        let t = tell(it, &raf)?;
        di.set("DataPos", pl::sub(&pl::sub(&t, &n(30)), &len));
        di.set("DataLen", pl::add(&n(30), &len));
        di.set("DirStart", n(0));
        di.set("DirLen", pl::add(&n(30), &len));
        di.set("MixedTags", n(1)); // (to ignore FileComment tag)
        pl::method(it, &et, "ProcessDirectory", vec![di, tag_table_ptr.clone()])?;
        buff = data_pt.deref();
        let bp = pl::sref(buff.clone());
        let flags = get16u(it, &bp, 6)?;
        if flags.to_i64() & 0x08 != 0 {
            // we don't yet support skipping stream mode data
            // (when this happens, the CRC, compressed size and uncompressed
            //  sizes are set to 0 in the header.  Instead, they are stored
            //  after the compressed data with an optional header of 0x08074b50)
            warn(it, &et, s("Stream mode data encountered, file list may be incomplete"))?;
            break;
        }
        len = get32u(it, &bp, 18)?; // file data length
        if !seek(it, &raf, len, 1)? {
            break; // skip file data
        }
        if !(read_eq(it, &raf, &mut buff, &n(30))? && re(r"^PK\x03\x04", "").is_match(&buff)) {
            break;
        }
    }
    et.delete("DOC_NUM");
    if doc_num > 1 && !options(it, &et, "Duplicates")?.truthy() {
        pl::method(it, &et, "Warn", vec![Value::from(format!("Use the Duplicates option to extract tags for all {doc_num} files")), n(1)])?;
    }
    ret(n(1))
}

pub fn register(it: &mut Interp) {
    it.define_native("Image::ExifTool::ZIP::WarnProc", warn_proc);
    it.define_native("Image::ExifTool::ZIP::ReadULEB", read_uleb);
    it.define_native("Image::ExifTool::ZIP::ProcessRAR", process_rar);
    it.define_native("Image::ExifTool::ZIP::ProcessGZIP", process_gzip);
    it.define_native("Image::ExifTool::ZIP::HandleMember", handle_member);
    it.define_native("Image::ExifTool::ZIP::ExtractFile", extract_file);
    it.define_native("Image::ExifTool::ZIP::ProcessZIP", process_zip);
}
