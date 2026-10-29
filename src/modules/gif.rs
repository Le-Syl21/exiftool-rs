//! Image::ExifTool::GIF (Image/ExifTool/GIF.pm), ported sub by sub (see docs/port/PORTING.md).
//!
//! Read and write GIF meta information

use crate::interp::eval::ER;
use crate::interp::{Interp, Want};
use crate::perl::Value;
use crate::pl::{self, arg, n, ret};

const MODULE: &str = "Image::ExifTool::GIF";

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

/// `print $out MSG`
fn print(it: &mut Interp, out: &Value, msg: Value) -> ER<()> {
    pl::print_to(it, Some(out), &[msg])?;
    Ok(())
}

/// `pos($dat)` after `$dat =~ /PAT/g` in scalar context (from position 0).
fn match_end(pat: &[u8], subject: &Value) -> ER<Option<usize>> {
    let rx = match crate::perl::regex::compile(pat, "") {
        Ok(r) => r,
        Err(e) => return crate::interp::eval::die(e),
    };
    Ok(match rx.match_at(&subject.to_bytes(), 0) {
        Ok(Some(sp)) => sp[0].map(|(_, e)| e),
        _ => None,
    })
}

/// `"\0" x $count`
fn nulls(count: &Value) -> Value {
    Value::bytes(vec![0u8; count.to_i64().max(0) as usize])
}

/// `pack('C*',1,reverse(0..255))` (the landing zone), plus `tail`.
fn landing_zone(tail: &[u8]) -> Value {
    let mut v = vec![1u8];
    v.extend((0..=255u8).rev());
    v.extend_from_slice(tail);
    Value::bytes(v)
}

/// Write a comment as an extension block, in 255-byte chunks each
/// beginning with a length byte.
fn write_comment(it: &mut Interp, outfile: &Value, comment: &Value, err: &mut Value) -> ER<()> {
    // write comment marker
    if !write(it, outfile, &[Value::bytes(b"\x21\xfe")])? {
        *err = n(1);
    }
    let len = pl::length(comment);
    // write out the comment in 255-byte chunks, each
    // chunk beginning with a length byte
    let mut nn = n(0);
    while pl::lt(&nn, &len) {
        let mut size = pl::sub(&len, &nn);
        if pl::gt(&size, &n(255)) {
            size = n(255);
        }
        let str_ = pl::substr(comment, nn.to_i64(), Some(size.to_i64()));
        let p = pl::pack("C", &[size])?;
        if !write(it, outfile, &[p, str_])? {
            *err = n(1);
        }
        nn = pl::add(&nn, &n(255));
    }
    if !write(it, outfile, &[Value::bytes(b"\0")])? {
        *err = n(1); // empty chunk as terminator
    }
    Ok(())
}

/// Read/write meta information in GIF image
/// Inputs: 0) ExifTool object reference, 1) Directory information ref
/// Returns: 1 on success, 0 if this wasn't a valid GIF file, or -1 if
///          an output file was specified and a write error occurred
pub fn process_gif(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info) = (arg(&a, 0), arg(&a, 1));
    let outfile = dir_info.get("OutFile");
    let raf = dir_info.get("RAF");
    let verbose = pl::method(it, &et, "Options", vec![pl::s("Verbose")])?;
    let out = pl::method(it, &et, "Options", vec![pl::s("TextOut")])?;
    let (mut s, mut ch, mut buff) = (Value::Undef, Value::Undef, Value::Undef);
    let mut length;
    let (mut err, mut new_comment, mut set_comment, mut nv_comment, mut new_ext) = (Value::Undef, Value::Undef, Value::Undef, Value::Undef, Value::Undef);
    let mut add_dirs = Value::Undef;
    let done_dir = Value::new_hash();
    let (mut frame_count, mut delay_time) = (n(0), n(0));

    // verify this is a valid GIF file
    pl::at(192);
    if !read_eq(it, &raf, &mut buff, &n(6))? {
        return ret(n(0));
    }
    let Some(m) = pl::re(r"^GIF(8[79]a)$", "").m(&buff) else {
        return ret(n(0));
    };
    if !read_eq(it, &raf, &mut s, &n(7))? {
        return ret(n(0));
    }

    let ver = pl::cap(&m, 1);
    let mut rtn_val = n(0);
    let tag_table_ptr = call(it, "GetTagTable", vec![pl::s("Image::ExifTool::GIF::Main")])?;
    let ext_table = call(it, "GetTagTable", vec![pl::s("Image::ExifTool::GIF::Extensions")])?;
    call(it, "SetByteOrder", vec![pl::s("II")])?;
    let gif_map = crate::runtime::lex_hash(it, MODULE, "gifMap");
    let app_extensions = crate::runtime::lex_array(it, MODULE, "appExtensions");

    if outfile.truthy() {
        // add any user-defined writable app extensions to the list
        let mut keys: Vec<Value> = ext_table.keys().into_iter().map(Value::from).collect();
        keys.sort_by(|x, y| x.to_bytes().cmp(&y.to_bytes()));
        for ext in keys {
            let ext_info = ext_table.get(ext.to_bytes());
            if !ext_info.is_hash() {
                continue;
            }
            if !(ext_info.get("SubDirectory").truthy() && ext_info.get("Writable").truthy() && !gif_map.get(ext_info.get("Name").to_bytes()).truthy()) {
                continue;
            }
            gif_map.set(ext_info.get("Name").to_bytes(), pl::s("GIF"));
            app_extensions.push(ext);
        }
        pl::method(it, &et, "InitWriteDirs", vec![gif_map.clone(), pl::s("XMP")])?; // make XMP the preferred group for GIF
        add_dirs = et.get("ADD_DIRS");
        // determine if we are editing the File:Comment tag
        let del_group = et.get("DEL_GROUP");
        let nv_ref = pl::sref(Value::Undef);
        new_comment = pl::method(it, &et, "GetNewValue", vec![pl::s("Comment"), nv_ref.clone()])?;
        nv_comment = nv_ref.deref();
        if nv_comment.truthy() || del_group.get("File").truthy() {
            set_comment = n(1);
        }
        // change to GIF 89a if adding comment, XMP or ICC_Profile
        pl::at(219);
        if !pl::hderef(&add_dirs)?.is_empty() || !new_comment.is_undef() {

            buff = pl::s("GIF89a");
        }
        if !write(it, &outfile, &[buff.clone(), s.clone()])? {
            err = n(1);
        }
        new_ext = pl::method(it, &et, "GetNewTagInfoHash", vec![ext_table.clone()])?;
    } else {
        pl::method(it, &et, "SetFileType", vec![])?; // set file type
        pl::method(it, &et, "HandleTag", vec![tag_table_ptr.clone(), pl::s("GIFVersion"), ver])?;
        pl::method(it, &et, "HandleTag", vec![tag_table_ptr.clone(), pl::s("ScreenDescriptor"), s.clone()])?;
    }
    let flags = call(it, "Get8u", vec![pl::sref(s.clone()), n(4)])?;
    if flags.to_i64() & 0x80 != 0 {
        // does this image contain a color table?
        // calculate color table size
        length = n(3 * (2i64 << (flags.to_i64() & 0x07)));
        if !read_eq(it, &raf, &mut buff, &length)? {
            return ret(n(0)); // skip color table
        }
        if outfile.truthy() && !write(it, &outfile, &[buff.clone()])? {
            err = n(1);
        }
    }
    //
    // loop through GIF blocks
    //
    'block: loop {
        if !read(it, &raf, &mut ch, n(1))?.truthy() {
            break;
        }
        // write out any new metadata now if this isn't an extension block
        if outfile.truthy() && ord(&ch).to_i64() != 0x21 {
            // write the comment first if necessary
            if !new_comment.is_undef() && nv_comment.get("IsCreating").truthy() {
                // write comment marker
                if !write(it, &outfile, &[Value::bytes(b"\x21\xfe")])? {
                    err = n(1);
                }
                if verbose.truthy() {
                    print(it, &out, pl::concat(&pl::concat(&pl::s("  + Comment = "), &new_comment), &pl::s("\n")))?;
                }
                let len = pl::length(&new_comment);
                // write out the comment in 255-byte chunks, each
                // chunk beginning with a length byte
                let mut nn = n(0);
                while pl::lt(&nn, &len) {
                    let mut size = pl::sub(&len, &nn);
                    if pl::gt(&size, &n(255)) {
                        size = n(255);
                    }
                    let str_ = pl::substr(&new_comment, nn.to_i64(), Some(size.to_i64()));
                    let p = pl::pack("C", &[size])?;
                    if !write(it, &outfile, &[p, str_])? {
                        err = n(1);
                    }
                    nn = pl::add(&nn, &n(255));
                }
                if !write(it, &outfile, &[Value::bytes(b"\0")])? {
                    err = n(1); // empty chunk as terminator
                }
                new_comment = Value::Undef;
                nv_comment = Value::Undef; // delete any other extraneous comments
                et.set("CHANGED", pl::add(&et.get("CHANGED"), &n(1))); // increment file changed flag
            }
            // add application extensions if necessary
            let mut new: Vec<Value> = new_ext.keys().into_iter().map(Value::from).collect();
            new.sort_by(|x, y| x.to_bytes().cmp(&y.to_bytes()));
            let mut list = app_extensions.items();
            list.extend(new);
            for ext in list {
                let ext_info = ext_table.get(ext.to_bytes());
                let name = ext_info.get("Name");
                if new_ext.get(ext.to_bytes()).truthy() {
                    new_ext.delete(ext.to_bytes());
                    done_dir.set(name.to_bytes(), n(1)); // (we wrote this as a block instead)
                    buff = pl::method(it, &et, "GetNewValue", vec![ext_info.clone()])?;
                    pl::method(it, &et, "VerboseValue", vec![pl::concat(&pl::s("+ GIF:"), &name), buff.clone()])?;
                } else if add_dirs.exists(name.to_bytes()) && done_dir.get(name.to_bytes()).is_undef() {
                    done_dir.set(name.to_bytes(), n(1));
                    let tbl = call(it, "GetTagTable", vec![ext_info.path(&["SubDirectory", "TagTable"])])?;
                    let dir_info = Value::new_hash();
                    dir_info.set("Parent", pl::s("GIF"));
                    if verbose.truthy() {
                        print(it, &out, pl::concat(&pl::concat(&pl::s("Creating "), &name), &pl::s(" application extension block:\n")))?;
                    }
                    buff = pl::method(it, &et, "WriteDirectory", vec![dir_info, tbl])?;
                } else {
                    continue;
                }
                if !buff.is_undef() && pl::length(&buff).truthy() {
                    et.set("CHANGED", pl::add(&et.get("CHANGED"), &n(1)));
                    if !write(it, &outfile, &[Value::bytes(b"\x21\xff\x0b"), pl::substr(&ext, 0, Some(8)), pl::substr(&ext, 9, Some(3))])? {
                        err = n(1);
                    }
                    let mut pos = n(0);
                    let incl = ext_table.get(ext.to_bytes()).get("IncludeLengthBytes");
                    if !incl.truthy() {
                        let _len = pl::length(&buff);
                        while pl::lt(&pos, &pl::length(&buff)) {
                            let mut nn = pl::sub(&pl::length(&buff), &pos);
                            if pl::gt(&nn, &n(255)) {
                                nn = n(255);
                            }
                            if !write(it, &outfile, &[chr(&nn), pl::substr(&buff, pos.to_i64(), Some(nn.to_i64()))])? {
                                err = n(1);
                            }
                            pos = pl::add(&pos, &nn);
                        }
                        if !write(it, &outfile, &[Value::bytes(b"\0")])? {
                            err = n(1); // write null terminator
                        }
                    } else if pl::lt(&incl, &n(2)) {
                        while pl::lt(&pos, &pl::length(&buff)) {
                            pos = pl::add(&pos, &pl::add(&ord(&pl::substr(&buff, pos.to_i64(), Some(1))), &n(1)));
                        }
                        // write data, null padding and terminator
                        let pad = nulls(&pl::add(&pl::sub(&pos, &pl::length(&buff)), &n(1)));
                        if !write(it, &outfile, &[buff.clone(), pad])? {
                            err = n(1);
                        }
                    } else {
                        // write data, landing zone and null terminator
                        if !write(it, &outfile, &[buff.clone(), landing_zone(&[0])])? {
                            err = n(1);
                        }
                    }
                    done_dir.set(name.to_bytes(), pl::add(&done_dir.get(name.to_bytes()), &n(1))); // set to 2 to indicate we added it
                } else if verbose.truthy() {
                    print(it, &out, pl::concat(&pl::concat(&pl::s("  -> no "), &name), &pl::s(" to add\n")))?;
                }
            }
        }
        if ord(&ch).to_i64() == 0x2c {
            frame_count = pl::add(&frame_count, &n(1));
            if outfile.truthy() && !write(it, &outfile, &[ch.clone()])? {
                err = n(1);
            }
            // image descriptor
            if !(read_eq(it, &raf, &mut buff, &n(8))? && read(it, &raf, &mut ch, n(1))?.truthy()) {
                break;
            }
            if outfile.truthy() && !write(it, &outfile, &[buff.clone(), ch.clone()])? {
                err = n(1);
            }
            if verbose.truthy() && !outfile.truthy() {
                let v = pl::unpack("v*", &buff)?;
                let msg = pl::concat(
                    &pl::concat(&pl::concat(&pl::concat(&pl::concat(&pl::concat(&pl::concat(&pl::concat(&pl::s("Image: left="), &arg(&v, 0)), &pl::s(" top=")), &arg(&v, 1)), &pl::s(" width=")), &arg(&v, 2)), &pl::s(" height=")), &arg(&v, 3)),
                    &pl::s("\n"),
                );
                print(it, &out, msg)?;
            }
            if ord(&ch).to_i64() & 0x80 != 0 {
                // does color table exist?
                length = n(3 * (2i64 << (ord(&ch).to_i64() & 0x07)));
                // skip the color table
                if !read_eq(it, &raf, &mut buff, &length)? {
                    break;
                }
                if outfile.truthy() && !write(it, &outfile, &[buff.clone()])? {
                    err = n(1);
                }
            }
            // skip "LZW Minimum Code Size" byte
            if !read(it, &raf, &mut buff, n(1))?.truthy() {
                break;
            }
            if outfile.truthy() && !write(it, &outfile, &[buff.clone()])? {
                err = n(1);
            }
            // skip image blocks
            loop {
                if !read(it, &raf, &mut ch, n(1))?.truthy() {
                    break;
                }
                if outfile.truthy() && !write(it, &outfile, &[ch.clone()])? {
                    err = n(1);
                }
                if !ord(&ch).truthy() {
                    break;
                }
                if !read(it, &raf, &mut buff, ord(&ch))?.truthy() {
                    break;
                }
                if outfile.truthy() && !write(it, &outfile, &[buff.clone()])? {
                    err = n(1);
                }
            }
            continue; // continue with next field
        }
        //               last if ord($ch) == 0x3b;  # normal end of GIF marker
        if ord(&ch).to_i64() != 0x21 {
            if outfile.truthy() {
                if !write(it, &outfile, &[ch.clone()])? {
                    err = n(1);
                }
                // copy the rest of the file
                while read(it, &raf, &mut buff, n(65536))?.truthy() {
                    if !write(it, &outfile, &[buff.clone()])? {
                        err = n(1);
                    }
                }
            }
            rtn_val = n(1);
            break;
        }
        // get extension block type/size
        if !read_eq(it, &raf, &mut s, &n(2))? {
            break;
        }
        // get marker and block size
        let v = pl::unpack("CC", &s)?;
        let a_ = arg(&v, 0);
        length = arg(&v, 1);

        if a_.to_i64() == 0xfe {
            // comment extension

            let mut comment = pl::s("");
            while length.truthy() {
                if !read_eq(it, &raf, &mut buff, &length)? {
                    break;
                }
                if !outfile.truthy() {
                    pl::method(it, &et, "VerboseDump", vec![pl::sref(buff.clone())])?;
                }
                // add buffer to comment string
                comment = pl::concat(&comment, &buff);
                if !read(it, &raf, &mut ch, n(1))?.truthy() {
                    break; // read next block header
                }
                length = ord(&ch); // get next block size
            }
            if length.truthy() {
                break; // was a read error if length isn't zero
            }
            if outfile.truthy() {
                let mut is_overwriting = Value::Undef;
                if set_comment.truthy() {
                    if nv_comment.truthy() {
                        is_overwriting = pl::method(it, &et, "IsOverwriting", vec![nv_comment.clone(), comment.clone()])?;
                        // get new comment again (may have been shifted)
                        if !new_comment.is_undef() {
                            new_comment = pl::method(it, &et, "GetNewValue", vec![nv_comment.clone()])?;
                        }
                    } else {
                        // group delete, or deleting additional comments after writing one
                        is_overwriting = n(1);
                    }
                }
                if is_overwriting.truthy() {
                    et.set("CHANGED", pl::add(&et.get("CHANGED"), &n(1))); // increment file changed flag
                    pl::method(it, &et, "VerboseValue", vec![pl::s("- GIF:Comment"), comment.clone()])?;
                    comment = new_comment.clone();
                    if !comment.is_undef() {
                        pl::method(it, &et, "VerboseValue", vec![pl::s("+ GIF:Comment"), comment.clone()])?;
                    }
                    nv_comment = Value::Undef; // just delete remaining comments
                } else {
                    set_comment = Value::Undef; // leave remaining comments alone
                }
                if !comment.is_undef() {
                    write_comment(it, &outfile, &comment, &mut err)?;
                }
                new_comment = Value::Undef; // don't write the new comment again
            } else {
                rtn_val = n(1);
                if comment.truthy() {
                    pl::method(it, &et, "FoundTag", vec![pl::s("Comment"), comment.clone()])?;
                }
                // assume no more than one comment in FastScan mode
                if pl::method(it, &et, "Options", vec![pl::s("FastScan")])?.truthy() {
                    break;
                }
            }
            continue;
        } else if a_.to_i64() == 0xff && length.to_i64() == 0x0b {
            // application extension

            if !read_eq(it, &raf, &mut buff, &length)? {
                break;
            }
            let hdr = pl::concat(&pl::concat(&ch, &s), &buff);
            // add "/" for readability
            let mut tag = pl::concat(&pl::concat(&pl::substr(&buff, 0, Some(8)), &pl::s("/")), &pl::substr(&buff, 8, None));
            pl::tr(&mut tag, r"\0-\x1f", "", "d"); // remove nulls and control characters
            if verbose.truthy() {
                print(it, &out, pl::concat(&pl::concat(&pl::s("Application Extension: "), &tag), &pl::s("\n")))?;
            }

            let ext_info = ext_table.get(tag.to_bytes());
            let (mut subdir, mut incl_len, mut just_copy, mut name) = (Value::Undef, Value::Undef, Value::Undef, Value::Undef);
            if ext_info.truthy() {
                if outfile.truthy() && new_ext.get(ext_info.get("TagID").to_bytes()).truthy() {
                    new_ext.delete(ext_info.get("TagID").to_bytes()); // don't create again
                    // (write as a block -- don't define $subdir)
                } else {
                    subdir = ext_info.get("SubDirectory");
                }
                incl_len = ext_info.get("IncludeLengthBytes");
                name = ext_info.get("Name");
                // rewrite as-is unless this is a writable
                if outfile.truthy() && !ext_info.get("Writable").truthy() {
                    just_copy = n(1);
                }
            } else if outfile.truthy() {
                just_copy = n(1);
            }
            if just_copy.truthy() && !write(it, &outfile, &[hdr.clone()])? {
                err = n(1);
            }

            // read the extension data
            let mut dat = pl::s("");
            loop {
                if !read(it, &raf, &mut ch, n(1))?.truthy() {
                    break 'block; // read next block header
                }
                length = ord(&ch);
                if !length.truthy() {
                    break; // get next block size
                }
                if !read_eq(it, &raf, &mut buff, &length)? {
                    break 'block;
                }
                if just_copy.truthy() && !write(it, &outfile, &[ch.clone(), buff.clone()])? {
                    err = n(1);
                }
                dat = pl::concat(&dat, &if incl_len.truthy() { pl::concat(&ch, &buff) } else { buff.clone() });
            }
            if just_copy.truthy() {
                if !write(it, &outfile, &[Value::bytes(b"\0")])? {
                    err = n(1);
                }
                continue;
            } else if incl_len.truthy() {
                // remove landing zone or padding
                let term = ext_info.get("Terminator");
                let tpos = if term.truthy() { match_end(&term.to_bytes(), &dat)? } else { None };
                if let Some(p) = tpos {
                    dat = pl::substr(&dat, 0, Some(p as i64));
                } else if let Some(p) = match_end(b"\\0", &dat)? {
                    dat = pl::substr(&dat, 0, Some(p as i64 - 1));
                }
            }
            if subdir.truthy() {
                let dir_info = Value::new_hash();
                let dat_ref = pl::sref(dat.clone());
                dir_info.set("DataPt", dat_ref.clone());
                dir_info.set("DataLen", pl::length(&dat));
                dir_info.set("DirLen", pl::length(&dat));
                dir_info.set("DirName", name.clone());
                dir_info.set("Parent", pl::s("GIF"));
                let sub_table = call(it, "GetTagTable", vec![subdir.get("TagTable")])?;
                if !outfile.truthy() {
                    pl::method(it, &et, "ProcessDirectory", vec![dir_info, sub_table])?;
                    continue;
                }
                if just_copy.truthy() {
                    continue;
                }
                if done_dir.get(name.to_bytes()).truthy() && pl::gt(&done_dir.get(name.to_bytes()), &n(1)) {
                    pl::method(it, &et, "Warn", vec![pl::concat(&pl::concat(&pl::s("Duplicate "), &name), &pl::s(" block created"))])?;
                }
                buff = pl::method(it, &et, "WriteDirectory", vec![dir_info, sub_table])?;
                dat = dat_ref.deref();
                if !buff.is_undef() {
                    if !pl::length(&buff).truthy() {
                        continue; // delete this extension if length is zero
                    }
                    dat = buff.clone();
                }
                done_dir.set(name.to_bytes(), n(1));
            } else if outfile.truthy() && !just_copy.truthy() {
                let nv_hash = pl::method(it, &et, "GetNewValueHash", vec![ext_info.clone()])?;
                if nv_hash.truthy() && pl::method(it, &et, "IsOverwriting", vec![nv_hash.clone(), dat.clone()])?.truthy() {
                    et.set("CHANGED", pl::add(&et.get("CHANGED"), &n(1)));
                    let val = pl::method(it, &et, "GetNewValue", vec![ext_info.clone()])?;
                    pl::method(it, &et, "VerboseValue", vec![pl::concat(&pl::s("- GIF:"), &name), dat.clone()])?;
                    if !(!val.is_undef() && pl::length(&val).truthy()) {
                        continue;
                    }
                    dat = val;
                    pl::method(it, &et, "VerboseValue", vec![pl::concat(&pl::s("+ GIF:"), &name), dat.clone()])?;
                    done_dir.set(name.to_bytes(), n(1)); // (possibly wrote dir as a block)
                }
            } else if !outfile.truthy() {
                pl::method(it, &et, "HandleTag", vec![ext_table.clone(), tag.clone(), dat.clone()])?;
                continue;
            }
            if !write(it, &outfile, &[hdr.clone()])? {
                err = n(1); // write extension header
            }
            if incl_len.truthy() {
                // check for null just to be safe
                if incl_len.truthy() && pl::re(r"\0", "").is_match(&dat) {
                    pl::method(it, &et, "Error", vec![pl::concat(&name, &pl::s(" contained NULL character"))])?;
                }
                if pl::gt(&incl_len, &n(1)) {
                    // add landing zone (without terminator, which will be added later)
                    if incl_len.truthy() {
                        dat = pl::concat(&dat, &landing_zone(&[]));
                    }
                } else {
                    // pad with nulls as required
                    let mut pos = n(0);
                    while pl::lt(&pos, &pl::length(&dat)) {
                        pos = pl::add(&pos, &pl::add(&ord(&pl::substr(&dat, pos.to_i64(), Some(1))), &n(1)));
                    }
                    dat = pl::concat(&dat, &nulls(&pl::sub(&pos, &pl::length(&dat))));
                }
                // write data and landing zone
                if !write(it, &outfile, &[dat.clone()])? {
                    err = n(1);
                }
            } else {
                // write as sub-blocks
                let mut pos = n(0);
                let len = pl::length(&dat);
                while pl::lt(&pos, &len) {
                    let mut nn = pl::sub(&len, &pos);
                    if pl::gt(&nn, &n(255)) {
                        nn = n(255);
                    }
                    if !write(it, &outfile, &[chr(&nn), pl::substr(&dat, pos.to_i64(), Some(nn.to_i64()))])? {
                        err = n(1);
                    }
                    pos = pl::add(&pos, &nn);
                }
            }
            if !write(it, &outfile, &[Value::bytes(b"\0")])? {
                err = n(1); // write null terminator
            }
            continue;
        } else if a_.to_i64() == 0xf9 && length.to_i64() == 4 {
            // graphic control extension

            if !read_eq(it, &raf, &mut buff, &length)? {
                break;
            }
            // sum the individual delay times
            let delay = call(it, "Get16u", vec![pl::sref(buff.clone()), n(1)])?;
            delay_time = pl::add(&delay_time, &delay);
            if verbose.truthy() {
                let msg = pl::sprintf("Graphic Control: delay=%.2f\n", &[pl::div(&delay, &n(100))?]);
                print(it, &out, msg)?;
            }
            // get transparent colour
            let bits = call(it, "Get8u", vec![pl::sref(buff.clone()), n(0)])?;
            if bits.to_i64() & 0x01 != 0 {
                let tc = call(it, "Get8u", vec![pl::sref(buff.clone()), n(3)])?;
                pl::method(it, &et, "HandleTag", vec![tag_table_ptr.clone(), pl::s("TransparentColor"), tc])?;
            }
            if !pl::method(it, &raf, "Seek", vec![pl::neg(&length), n(1)])?.truthy() {
                break;
            }
        } else if a_.to_i64() == 0x01 && length.to_i64() == 12 {
            // plain text extension

            if !read_eq(it, &raf, &mut buff, &length)? {
                break;
            }
            if outfile.truthy() && !write(it, &outfile, &[ch.clone(), s.clone(), buff.clone()])? {
                err = n(1);
            }
            if verbose.truthy() && !outfile.truthy() {
                let v = pl::unpack("v4", &buff)?;
                let msg = pl::concat(
                    &pl::concat(&pl::concat(&pl::concat(&pl::concat(&pl::concat(&pl::concat(&pl::concat(&pl::s("Text: left="), &arg(&v, 0)), &pl::s(" top=")), &arg(&v, 1)), &pl::s(" width=")), &arg(&v, 2)), &pl::s(" height=")), &arg(&v, 3)),
                    &pl::s("\n"),
                );
                print(it, &out, msg)?;
            }
            let mut text = pl::s("");
            loop {
                if !read(it, &raf, &mut ch, n(1))?.truthy() {
                    break;
                }
                length = ord(&ch);
                if !length.truthy() {
                    break;
                }
                if !read_eq(it, &raf, &mut buff, &length)? {
                    break;
                }
                if outfile.truthy() && !write(it, &outfile, &[ch.clone(), buff.clone()])? {
                    err = n(1); // write block
                }
                text = pl::concat(&text, &buff);
            }
            if outfile.truthy() && !write(it, &outfile, &[Value::bytes(b"\0")])? {
                err = n(1); // write terminator block
            }
            pl::method(it, &et, "HandleTag", vec![tag_table_ptr.clone(), pl::s("Text"), text])?;
            continue;
        }
        if outfile.truthy() && !write(it, &outfile, &[ch.clone(), s.clone()])? {
            err = n(1);
        }
        // skip the block
        while length.truthy() {
            if !read_eq(it, &raf, &mut buff, &length)? {
                break;
            }
            if outfile.truthy() && !write(it, &outfile, &[buff.clone()])? {
                err = n(1);
            }
            if !read(it, &raf, &mut ch, n(1))?.truthy() {
                break; // read next block header
            }
            if outfile.truthy() && !write(it, &outfile, &[ch.clone()])? {
                err = n(1);
            }
            length = ord(&ch); // get next block size
        }
    }
    if !outfile.truthy() {
        if pl::gt(&frame_count, &n(1)) {
            pl::method(it, &et, "HandleTag", vec![tag_table_ptr.clone(), pl::s("FrameCount"), frame_count])?;
        }
        if delay_time.truthy() {
            pl::method(it, &et, "HandleTag", vec![tag_table_ptr.clone(), pl::s("Duration"), pl::div(&delay_time, &n(100))?])?;
        }
    }

    // set return value to -1 if we only had a write error
    if rtn_val.truthy() && err.truthy() {
        rtn_val = n(-1);
    }
    ret(rtn_val)
}

/// Register the subs ported so far; the others still run from the Perl.
pub fn register(it: &mut Interp) {
    it.define_native("Image::ExifTool::GIF::ProcessGIF", process_gif);
}
