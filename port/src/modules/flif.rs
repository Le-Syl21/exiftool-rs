//! Image::ExifTool::FLIF (Image/ExifTool/FLIF.pm), ported sub by sub (see docs/port/PORTING.md).
//!
//! Read/write FLIF meta information

use crate::interp::eval::ER;
use crate::interp::{Interp, Want};
use crate::perl::{ops, Value};
use crate::pl::{self, arg, n, re};

const MODULE: &str = "Image::ExifTool::FLIF";

/// `Image::ExifTool::NAME(ARGS)` (the functions imported with :DataAccess :Utils).
fn call(it: &mut Interp, name: &str, args: Vec<Value>) -> ER<Value> {
    pl::call(it, &format!("Image::ExifTool::{name}"), args)
}

/// `$raf->Read($buff, $len)`: the count, with `buff` updated.
fn read(it: &mut Interp, raf: &Value, buff: &mut Value, len: Value) -> ER<Value> {
    let mut args = [buff.clone(), len];
    let r = pl::method_mut(it, raf, "Read", &mut args, Want::Scalar)?;
    *buff = args[0].clone();
    Ok(r.into_iter().next().unwrap_or_default())
}

/// `$raf->Read($buff, $len) == $len`
fn read_eq(it: &mut Interp, raf: &Value, buff: &mut Value, len: &Value) -> ER<bool> {
    let r = read(it, raf, buff, len.clone())?;
    Ok(pl::num_eq(&r, len))
}

/// `Write($outfile, LIST)`: true on success.
fn write(it: &mut Interp, outfile: &Value, items: &[Value]) -> ER<bool> {
    let mut args = vec![outfile.clone()];
    args.extend_from_slice(items);
    Ok(call(it, "Write", args)?.truthy())
}

/// `ord($ch)`
fn ord(v: &Value) -> Value {
    n(v.to_bytes().first().map_or(0, |b| *b as i64))
}

/// `chr($n)` (for values 0-255)
fn chr(v: &Value) -> Value {
    Value::bytes([v.to_i64() as u8])
}

/// `eval { require MODULE }`: true if it loaded.
fn eval_require(it: &mut Interp, what: &str) -> ER<bool> {
    Ok(pl::eval_block(it, |it| crate::runtime::require(it, what))?.is_some())
}

/// `PKG::FUNC(\$in => \$out)`: the result, with `out` updated.
fn zlib_call(it: &mut Interp, func: &str, input: &Value, out: &mut Value) -> ER<Value> {
    let out_ref = pl::sref(out.clone());
    let r = pl::call(it, func, vec![input.clone(), out_ref.clone()])?;
    *out = out_ref.deref();
    Ok(r)
}

/// Read variable-length FLIF integer
/// Inputs: 0) raf reference, 1) number to add to returned value
/// Returns: integer, or undef on EOF
pub fn get_var_int(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (raf, add) = (arg(&a, 0), arg(&a, 1));
    let mut buff = Value::Undef;
    let mut val = n(0);
    loop {
        pl::at(119);
        if !read(it, &raf, &mut buff, n(1))?.truthy() {
            return pl::ret_undef();
        }
        let byte = ord(&buff);
        val = ops::bit_or(&val, &ops::bit_and(&byte, &n(0x7f)));
        if !ops::bit_and(&byte, &n(0x80)).truthy() {
            break;
        }
        val = ops::shift_left(&val, &n(7));
    }
    pl::ret(pl::add(&val, &if add.truthy() { add } else { n(0) }))
}

fn get_var_int1(it: &mut Interp, raf: &Value, add: Value) -> ER<Value> {
    Ok(get_var_int(it, vec![raf.clone(), add], Want::Scalar)?
        .pop()
        .unwrap_or_default())
}

/// Construct variable-length FLIF integer
/// Inputs: 0) integer
/// Returns: FLIF variable-length integer byte stream
pub fn set_var_int(_it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let mut val = arg(&a, 0);
    let mut buff = pl::s("");
    let mut high = n(0);
    loop {
        buff = pl::concat(
            &chr(&ops::bit_or(&ops::bit_and(&val, &n(0x7f)), &high)),
            &buff,
        );
        val = ops::shift_right(&val, &n(7));
        if !val.truthy() {
            break;
        }
        high = n(0x80);
    }
    pl::ret(buff)
}

fn set_var_int1(it: &mut Interp, val: Value) -> ER<Value> {
    Ok(set_var_int(it, vec![val], Want::Scalar)?
        .pop()
        .unwrap_or_default())
}

/// Read FLIF header
/// Inputs: 0) RAF ref
/// Returns: Scalar context: binary header block
///          List context: header values (4 or 5 elements: type,depth,width,height[,frames])
///          or undef if this isn't a valid FLIF file header
pub fn read_flif_header(it: &mut Interp, a: Vec<Value>, want: Want) -> ER<Vec<Value>> {
    let raf = arg(&a, 0);
    let mut buff = Value::Undef;
    let mut vals: Vec<Value> = Vec::new();

    // verify this is a valid FLIF file
    pl::at(156);
    if !read_eq(it, &raf, &mut buff, &n(6))? {
        return pl::ret_nothing(want);
    }
    let Some(m) = re(r"^FLIF([0-\x6f])([0-2])", "").m(&buff) else {
        return pl::ret_nothing(want);
    };

    // decode header information ("FLIF" chunk)
    vals.push(pl::cap(&m, 1)); // type, depth
    vals.push(pl::cap(&m, 2));
    vals.push(get_var_int1(it, &raf, n(1))?); // width, height (+1 each)
    vals.push(get_var_int1(it, &raf, n(1))?);
    if vals[0].to_bytes()[..] > b"H"[..] {
        vals.push(get_var_int1(it, &raf, n(2))?); // frames (+2)
    }

    if vals.last().unwrap().is_undef() {
        return pl::ret_nothing(want);
    }
    if want == Want::List {
        return Ok(vals); // return the decoded header values
    }

    // return the binary header block
    pl::at(167);
    let hdr_len = pl::method(it, &raf, "Tell", vec![])?;
    pl::at(168);
    if !(pl::method(it, &raf, "Seek", vec![n(0), n(0)])?.truthy()
        && read_eq(it, &raf, &mut buff, &hdr_len)?)
    {
        return pl::ret_nothing(want);
    }
    pl::ret(buff)
}

/// WriteFLIF : Write FLIF image
/// Inputs: 0) ExifTool object reference, 1) dirInfo reference
/// Returns: 1 on success, 0 if this wasn't a valid FLIF file, or -1 if
///          an output file was specified and a write error occurred
pub fn write_flif(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info) = (arg(&a, 0), arg(&a, 1));
    let raf = dir_info.get("RAF");
    let mut soi = Value::Undef;
    let mut add_tags: Vec<Value> = Vec::new();
    let done_tag = Value::new_hash();

    // verify FLIF header and copy it to the output file
    let mut buff = read_flif_header(it, vec![raf.clone()], Want::Scalar)?
        .pop()
        .unwrap_or_default();
    if !buff.truthy() {
        return pl::ret(n(0));
    }
    let outfile = dir_info.get("OutFile");
    if !write(it, &outfile, &[buff.clone()])? {
        return pl::ret(n(-1));
    }

    let flif_map = crate::runtime::lex_hash(it, MODULE, "flifMap");
    pl::method(it, &et, "InitWriteDirs", vec![flif_map])?;
    let tag_table_ptr = call(
        it,
        "GetTagTable",
        vec![pl::s("Image::ExifTool::FLIF::Main")],
    )?;

    // loop through the FLIF chunks
    loop {
        let tag;
        let size;
        let mut inflated = Value::Undef;
        // read new tag (or soi) unless we already hit the soi (start of image)
        if soi.is_undef() {
            if !read_eq(it, &raf, &mut buff, &n(4))? {
                pl::method(it, &et, "Error", vec![pl::s("Unexpected EOF")])?;
                break;
            }
            if buff.to_bytes()[..] < b" "[..] {
                soi = buff.clone(); // we have hit the start of image (no more metadata)
                                    // make list of new tags to add
                for t in ["eXif", "eXmp", "iCCP"] {
                    let name = tag_table_ptr.get(t).get("Name");
                    if et.get("ADD_DIRS").get(name.to_bytes()).truthy() && !done_tag.get(t).truthy()
                    {
                        add_tags.push(pl::s(t));
                    }
                }
            }
        }
        if soi.is_undef() {
            tag = buff.clone();
            size = get_var_int1(it, &raf, Value::Undef)?; // read the data size
        } else if !add_tags.is_empty() {
            tag = add_tags.remove(0);
            buff = pl::s(""); // create metadata from scratch
            size = n(0);
        } else {
            // finish copying file (no more metadata to add)
            if !write(it, &outfile, &[soi.clone()])? {
                return pl::ret(n(-1));
            }
            while read(it, &raf, &mut buff, n(65536))?.truthy() {
                if !write(it, &outfile, &[buff.clone()])? {
                    return pl::ret(n(-1));
                }
            }
            break; // all done!
        }
        let tag_info = pl::method(
            it,
            &et,
            "GetTagInfo",
            vec![tag_table_ptr.clone(), tag.clone()],
        )?;
        if tag_info.truthy()
            && tag_info.get("SubDirectory").truthy()
            && et
                .get("EDIT_DIRS")
                .get(tag_info.get("Name").to_bytes())
                .truthy()
        {
            done_tag.set(tag.to_bytes(), n(1)); // prevent adding this back again later
            if soi.is_undef() && !read_eq(it, &raf, &mut buff, &size)? {
                pl::method(
                    it,
                    &et,
                    "Error",
                    vec![cat(&[
                        pl::s("Truncated FLIF "),
                        tag.clone(),
                        pl::s(" chunk"),
                    ])],
                )?;
                break;
            }
            // rewrite the compressed data
            if eval_require(it, "IO::Uncompress::RawInflate")?
                && eval_require(it, "IO::Compress::RawDeflate")?
            {
                if pl::length(&buff).to_i64() == 0 {
                    inflated = buff.clone(); // (creating from scratch, so no need to inflate)
                } else if !zlib_call(
                    it,
                    "IO::Uncompress::RawInflate::rawinflate",
                    &pl::sref(buff.clone()),
                    &mut inflated,
                )?
                .truthy()
                {
                    pl::method(
                        it,
                        &et,
                        "Error",
                        vec![cat(&[
                            pl::s("Error inflating FLIF "),
                            tag.clone(),
                            pl::s(" chunk"),
                        ])],
                    )?;
                    break;
                }
                let subdir = tag_info.get("SubDirectory");
                let subdir_info = Value::new_hash();
                subdir_info.set("DirName", tag_info.get("Name"));
                subdir_info.set("DataPt", pl::sref(inflated.clone()));
                subdir_info.set(
                    "DirStart",
                    if pl::length(&inflated).truthy() {
                        subdir.get("Start")
                    } else {
                        Value::Undef
                    },
                );
                subdir_info.set("ReadOnly", n(1)); // (used only by WriteXMP)
                let sub_table = call(it, "GetTagTable", vec![subdir.get("TagTable")])?;
                inflated = pl::method(
                    it,
                    &et,
                    "WriteDirectory",
                    vec![subdir_info, sub_table, subdir.get("WriteProc")],
                )?;
                if !inflated.is_undef() {
                    if !pl::length(&inflated).truthy() {
                        continue; // (delete directory if length is zero)
                    }
                    if subdir.get("Header").truthy() {
                        inflated = pl::concat(&subdir.get("Header"), &inflated);
                        // (add back header if necessary)
                    }
                    if !zlib_call(
                        it,
                        "IO::Compress::RawDeflate::rawdeflate",
                        &pl::sref(inflated.clone()),
                        &mut buff,
                    )?
                    .truthy()
                    {
                        pl::method(
                            it,
                            &et,
                            "Error",
                            vec![cat(&[
                                pl::s("Error deflating FLIF "),
                                tag.clone(),
                                pl::s(" chunk"),
                            ])],
                        )?;
                        break;
                    }
                }
            } else {
                pl::method(
                    it,
                    &et,
                    "Warn",
                    vec![pl::s(
                        "Install IO::Compress::RawDeflate to write FLIF metadata",
                    )],
                )?;
            }
            let len = set_var_int1(it, pl::length(&buff))?;
            if !write(it, &outfile, &[tag.clone(), len, buff.clone()])? {
                return pl::ret(n(-1));
            }
        } else if soi.is_undef() {
            let len = set_var_int1(it, size.clone())?;
            if !write(it, &outfile, &[tag.clone(), len])? {
                return pl::ret(n(-1));
            }
            if !call(
                it,
                "CopyBlock",
                vec![raf.clone(), outfile.clone(), size.clone()],
            )?
            .truthy()
            {
                return pl::ret(n(-1));
            }
        }
    }
    pl::ret(n(1))
}

/// `"a" . "b" . ...`
fn cat(parts: &[Value]) -> Value {
    let mut v = Vec::new();
    for p in parts {
        v.extend_from_slice(&p.to_bytes());
    }
    Value::bytes(v)
}

/// Extract information from an FLIF file
/// Inputs: 0) ExifTool object reference, 1) DirInfo reference
/// Returns: 1 on success, 0 if this wasn't a valid FLIF file
pub fn process_flif(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info) = (arg(&a, 0), arg(&a, 1));
    let raf = dir_info.get("RAF");
    let mut buff = Value::Undef;
    let mut tag = Value::Undef;
    let mut inflated = Value::Undef;

    // verify this is a valid FLIF file and read the header
    let vals = read_flif_header(it, vec![raf.clone()], Want::List)?;
    if vals.is_empty() {
        return pl::ret(n(0));
    }

    pl::method(it, &et, "SetFileType", vec![])?;
    let tag_table_ptr = call(
        it,
        "GetTagTable",
        vec![pl::s("Image::ExifTool::FLIF::Main")],
    )?;
    let verbose = pl::method(it, &et, "Options", vec![pl::s("Verbose")])?;

    // save the header information
    if verbose.truthy() {
        pl::method(it, &et, "VPrint", vec![n(0), pl::s("FLIF header:\n")])?;
    }
    let mut t = 0;
    while t < vals.len() && !vals[t].is_undef() {
        pl::method(
            it,
            &et,
            "HandleTag",
            vec![tag_table_ptr.clone(), n(t as i64), vals[t].clone()],
        )?;
        t += 1;
    }

    // loop through the FLIF chunks
    loop {
        if !read_eq(it, &raf, &mut tag, &n(4))? {
            pl::method(it, &et, "Warn", vec![pl::s("Unexpected EOF")])?;
            break;
        }
        let byte = ord(&pl::substr(&tag, 0, Some(1)));
        // all done if we arrived at the image chunk
        if pl::lt(&byte, &n(32)) {
            pl::method(
                it,
                &et,
                "HandleTag",
                vec![tag_table_ptr.clone(), n(5), byte],
            )?;
            break;
        }
        let size = get_var_int1(it, &raf, Value::Undef)?;
        if verbose.truthy() {
            pl::method(
                it,
                &et,
                "VPrint",
                vec![
                    n(0),
                    cat(&[
                        pl::s("FLIF "),
                        tag.clone(),
                        pl::s(" ("),
                        size.clone(),
                        pl::s(" bytes):\n"),
                    ]),
                ],
            )?;
        }
        if tag_table_ptr.get(tag.to_bytes()).truthy() {
            if !read_eq(it, &raf, &mut buff, &size)? {
                pl::method(
                    it,
                    &et,
                    "Warn",
                    vec![cat(&[
                        pl::s("Truncated FLIF "),
                        tag.clone(),
                        pl::s(" chunk"),
                    ])],
                )?;
                break;
            }
            if pl::gt(&verbose, &n(2)) {
                let addr = pl::sub(&pl::method(it, &raf, "Tell", vec![])?, &size);
                pl::method(
                    it,
                    &et,
                    "VerboseDump",
                    vec![pl::sref(buff.clone()), pl::s("Addr"), addr],
                )?;
            }
            // inflate the compressed data
            if eval_require(it, "IO::Uncompress::RawInflate")? {
                if zlib_call(
                    it,
                    "IO::Uncompress::RawInflate::rawinflate",
                    &pl::sref(buff.clone()),
                    &mut inflated,
                )?
                .truthy()
                {
                    pl::method(
                        it,
                        &et,
                        "HandleTag",
                        vec![
                            tag_table_ptr.clone(),
                            tag.clone(),
                            inflated.clone(),
                            pl::s("DataPt"),
                            pl::sref(inflated.clone()),
                            pl::s("Size"),
                            pl::length(&inflated),
                            pl::s("Extra"),
                            pl::s(" inflated"),
                        ],
                    )?;
                } else {
                    pl::method(
                        it,
                        &et,
                        "Warn",
                        vec![cat(&[
                            pl::s("Error inflating FLIF "),
                            tag.clone(),
                            pl::s(" chunk"),
                        ])],
                    )?;
                }
            } else {
                pl::method(
                    it,
                    &et,
                    "Warn",
                    vec![pl::s(
                        "Install IO::Uncompress::RawInflate to decode FLIF metadata",
                    )],
                )?;
            }
        } else if !pl::method(it, &raf, "Seek", vec![size.clone(), n(1)])?.truthy() {
            pl::method(it, &et, "Warn", vec![pl::s("Seek error")])?;
            break;
        }
    }
    pl::ret(n(1))
}

pub fn register(it: &mut Interp) {
    it.define_native("Image::ExifTool::FLIF::GetVarInt", get_var_int);
    it.define_native("Image::ExifTool::FLIF::SetVarInt", set_var_int);
    it.define_native("Image::ExifTool::FLIF::ReadFLIFHeader", read_flif_header);
    it.define_native("Image::ExifTool::FLIF::WriteFLIF", write_flif);
    it.define_native("Image::ExifTool::FLIF::ProcessFLIF", process_flif);
}
