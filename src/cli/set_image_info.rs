//! The `exiftool` application script, ported sub by sub (see docs/port/CLI.md).
//! This part: `SetImageInfo` (script lines 3131-3681), the writing of one file.

use std::cell::RefCell;
use std::rc::Rc;

use crate::interp::eval::{Code, ER};
use crate::interp::{Flow, Interp, Want};
use crate::perl::Value;
use crate::pl::{self, arg, cap, re, ret};

// ---- local helpers (Perl idioms not in crate::pl) ----

fn n(i: i64) -> Value {
    Value::Int(i)
}

fn s(x: &str) -> Value {
    Value::str(x)
}

/// A string built from parts, as a Perl interpolation ("a$b c").
fn cat(parts: &[Value]) -> Value {
    let mut out: Vec<u8> = Vec::new();
    for p in parts {
        out.extend_from_slice(&p.to_bytes());
    }
    Value::bytes(out)
}

/// The value of the script's file lexical `$name`.
fn get(it: &mut Interp, name: &str) -> Value {
    super::lex_scalar(it, name).borrow().clone()
}

/// `$name = v` for the script's file lexical `$name`.
fn set(it: &mut Interp, name: &str, v: Value) {
    *super::lex_scalar(it, name).borrow_mut() = v;
}

/// `++$name` for a file lexical (a counter).
fn inc(it: &mut Interp, name: &str) {
    let c = super::lex_scalar(it, name);
    let v = pl::add(&c.borrow(), &n(1));
    *c.borrow_mut() = v;
}

/// A script sub called as a statement (void context).
fn call_void(it: &mut Interp, name: &str, args: Vec<Value>) -> ER<()> {
    it.call_named(name, args, Want::Void)?;
    Ok(())
}

/// A method called as a statement (void context).
fn meth_void(it: &mut Interp, obj: &Value, name: &str, args: Vec<Value>) -> ER<()> {
    let mut a = vec![obj.clone()];
    a.extend(args);
    it.call_method(obj, name, a, Want::Void)?;
    Ok(())
}

/// `Warn LIST` (the script's own Warn).
fn warn_(it: &mut Interp, msg: Value) -> ER<()> {
    call_void(it, "main::Warn", vec![msg])
}

/// `EFile($infile)` / `EFile($infile, CODE)`.
fn efile(it: &mut Interp, infile: &Value, code: Option<i64>) -> ER<()> {
    let mut a = vec![infile.clone()];
    if let Some(c) = code {
        a.push(n(c));
    }
    call_void(it, "main::EFile", a)
}

/// `print $vout LIST`.
fn print_vout(it: &mut Interp, items: &[Value]) -> ER<()> {
    let vout = get(it, "vout");
    pl::print_to(it, Some(&vout), items)?;
    Ok(())
}

/// One Perl statement on the script's bareword file handles (NEW_FILE,
/// ORIG_FILE, STDOUT), which natives cannot name: run in package main with
/// `vars` as its lexicals. Returns the statement's value and the variables
/// afterwards. `$@` is left as it was, as the statement itself leaves it.
fn handle_op(it: &mut Interp, code: &str, vars: &[(&str, Value)]) -> ER<(Value, Vec<Value>)> {
    let scope = crate::interp::Scope::new(None);
    let cells: Vec<_> = vars.iter().map(|(k, v)| scope.scalar(k, v.clone())).collect();
    let saved = it.get_scalar("main::@");
    let r = it.eval_string(code.as_bytes(), "main", &scope, Want::Scalar);
    it.set_scalar("main::@", saved);
    match r {
        Ok(v) => Ok((v.last().cloned().unwrap_or_default(), cells.iter().map(|c| c.borrow().clone()).collect())),
        Err(e) => Err(Flow::Die(e)),
    }
}

/// `%$h` in list context (the key/value pairs).
fn hash_pairs(h: &Value) -> Vec<Value> {
    let mut out = Vec::new();
    if let Value::HashRef(rc) = h {
        for (k, v) in rc.borrow().iter() {
            out.push(Value::bytes(k));
            out.push(v.clone());
        }
    }
    out
}

/// `ref $x eq 'ARRAY'`-style test.
fn is_ref_type(v: &Value, t: &str) -> bool {
    pl::ref_of(v) == t
}

//------------------------------------------------------------------------------
// Set information in file
// Inputs: 0) ExifTool object reference, 1) source file name
//         2) original source file name ('' to create from scratch)
// Returns: true on success
pub fn set_image_info(it: &mut Interp, a: Vec<Value>, _want: Want) -> ER<Vec<Value>> {
    let (et, mut file, orig) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    let (mut outfile, mut restored, mut is_temporary, mut is_stdout, mut out_type, mut tags_from_src) =
        (Value::Undef, false, false, false, Value::Undef, Value::Undef);
    let (mut hard_link, mut sym_link, mut test_name, mut same_file) = (Value::Undef, Value::Undef, Value::Undef, Value::Undef);
    let infile = file.clone(); // save infile in case we change it again

    // clean up old temporary file if necessary
    if !get(it, "tmpFile").is_undef() {
        let tmp = get(it, "tmpFile");
        meth_void(it, &et, "Unlink", vec![tmp])?;
        set(it, "tmpFile", Value::Undef);
    }
    // clear any existing errors or warnings since we check these on return
    let value = et.get("VALUE");
    value.delete("Error");
    value.delete("Warning");

    let tags = super::lex_array(it, "tags");
    let set_tags_h = super::lex_hash(it, "setTags");
    let do_set_file_name = get(it, "doSetFileName");

    // first, try to determine our output file name so we can return quickly
    // if it already exists (note: this test must be delayed until after we
    // set tags from dynamic files if writing FileName or Directory)
    let out_opt = get(it, "outOpt");
    if !out_opt.is_undef() {
        if let Some(m) = re(r"^-(\.\w+)?$", "").m(&out_opt) {
            // allow output file type to be specified with "-o -.EXT"
            if cap(&m, 1).truthy() {
                out_type = pl::call(it, "Image::ExifTool::GetFileType", vec![out_opt.clone()])?;
            }
            outfile = s("-");
            is_stdout = true;
        } else {
            outfile = pl::call(it, "main::FilenameSPrintf", vec![out_opt.clone(), orig.clone()])?;
            // (perl warns here if $outfile is undef)
            if pl::str_eq(&outfile, "") {
                warn_(it, cat(&[s("Error: Can't create file with zero-length name from "), orig.clone(), s("\n")]))?;
                efile(it, &infile, None)?;
                inc(it, "countBadCr");
                return ret(n(0));
            }
        }
        let list_dir = get(it, "listDir");
        if !is_stdout
            && ((pl::method(it, &et, "IsDirectory", vec![outfile.clone()])?.truthy() && !list_dir.truthy())
                || re(r"/$", "").is_match(&outfile))
        {
            if !re(r"/$", "").is_match(&outfile) {
                outfile = pl::concat(&outfile, &s("/"));
            }
            let mut name = file.clone();
            re(r"^.*/", "s").subst(&mut name, false, |_| Vec::new()); // remove directory name
            outfile = pl::concat(&outfile, &name);
        } else {
            let st = pl::call(it, "Image::ExifTool::GetFileType", vec![file.clone()])?;
            let src_type = if st.truthy() { st } else { s("") };
            if !out_type.truthy() {
                out_type = pl::call(it, "Image::ExifTool::GetFileType", vec![outfile.clone()])?;
            }
            if out_type.truthy()
                && (src_type.to_bytes() != out_type.to_bytes() || pl::str_eq(&out_type, "ICC"))
                && !pl::str_eq(&file, "-")
            {
                if !pl::call(it, "Image::ExifTool::CanCreate", vec![out_type.clone()])?.truthy() {
                    let what = if src_type.truthy() { s("other types") } else { s("scratch") };
                    call_void(it, "main::WarnOnce", vec![cat(&[s("Error: Can't create "), out_type.clone(), s(" files from "), what, s("\n")])])?;
                    efile(it, &infile, None)?;
                    inc(it, "countBadCr");
                    return ret(n(0));
                }
                if !pl::str_eq(&file, "") {
                    // restore previous new values unless done already
                    if !restored {
                        meth_void(it, &et, "RestoreNewValues", vec![])?;
                    }
                    restored = true;
                    // translate to this type by setting specified tags from file
                    let mut set_tags: Vec<Value> = tags.items();
                    for x in super::lex_array(it, "exclude").items() {
                        set_tags.push(pl::concat(&s("-"), &x));
                    }
                    // force some tags to be copied for certain file types
                    let force_copy = match &*out_type.to_bytes() {
                        b"ICC" => Some("ICC_Profile"),
                        b"VRD" => Some("CanonVRD"),
                        b"DR4" => Some("CanonDR4"),
                        _ => None,
                    };
                    if let Some(fc) = force_copy {
                        set_tags.push(s(fc));
                    }
                    // assume "-tagsFromFile @" unless -tagsFromFile already specified
                    // (%setTags won't be empty if -tagsFromFile used)
                    if set_tags_h.keys().is_empty() || (!set_tags.is_empty() && !set_tags_h.get("@").truthy()) {
                        let r = pl::call(it, "main::DoSetFromFile", vec![et.clone(), file.clone(), Value::new_array(set_tags)])?;
                        if !r.truthy() {
                            return ret(n(0));
                        }
                    } else if !set_tags.is_empty() {
                        // add orphaned tags to existing "-tagsFromFile @" for this file only
                        pl::at(3205);
                        set_tags.extend(pl::aderef(&set_tags_h.get("@"))?);
                        tags_from_src = Value::new_array(set_tags);
                    }
                    // all done with source file -- create from meta information alone
                    file = s("");
                }
            }
        }
        if !is_stdout {
            outfile = pl::call(it, "main::NextUnusedFilename", vec![outfile.clone()])?;
            if pl::method(it, &et, "Exists", vec![outfile.clone(), n(1)])?.truthy() && !do_set_file_name.truthy() {
                warn_(it, cat(&[s("Error: '"), outfile.clone(), s("' already exists - "), infile.clone(), s("\n")]))?;
                efile(it, &infile, None)?;
                inc(it, "countBadWr");
                return ret(n(0));
            }
        }
    } else if pl::str_eq(&file, "-") {
        is_stdout = true;
    }
    // set tags from destination file if required
    let dynamic_files = super::lex_array(it, "dynamicFiles");
    if !dynamic_files.is_empty() {
        // restore previous values if necessary
        if !restored {
            meth_void(it, &et, "RestoreNewValues", vec![])?;
        }
        let set_tags_index = Value::new_hash();
        let set_tags_list = super::lex_hash(it, "setTagsList");
        for dy_file in dynamic_files.items() {
            if !dy_file.is_ref() {
                let (from_file, mut set_tags);
                if pl::str_eq(&dy_file, "@") {
                    from_file = orig.clone();
                    set_tags = if tags_from_src.truthy() { tags_from_src.clone() } else { set_tags_h.get(dy_file.to_bytes()) };
                } else {
                    from_file = pl::call(it, "main::FilenameSPrintf", vec![dy_file.clone(), orig.clone()])?;
                    if from_file.is_undef() {
                        efile(it, &infile, None)?;
                        inc(it, "countBadWr");
                        return ret(n(0));
                    }
                    set_tags = set_tags_h.get(dy_file.to_bytes());
                }
                // do we have multiple -tagsFromFile options with this file?
                let key = dy_file.to_bytes();
                if set_tags_list.get(&key).truthy() {
                    // use the tags set in the i-th occurrence
                    let iv = set_tags_index.get(&key);
                    let i = if iv.truthy() { iv } else { n(0) };
                    set_tags_index.set(&key, pl::add(&i, &n(1)));
                    let lst = set_tags_list.get(&key).at(i.to_i64());
                    if lst.truthy() {
                        set_tags = lst;
                    }
                }
                // set new values values from file
                let r = pl::call(it, "main::DoSetFromFile", vec![et.clone(), from_file, set_tags])?;
                if !r.truthy() {
                    return ret(n(0));
                }
            } else if is_ref_type(&dy_file, "ARRAY") {
                // a dynamic file containing a simple tag value
                let fname = pl::call(it, "main::FilenameSPrintf", vec![dy_file.at(1), orig.clone()])?;
                let buff: Rc<RefCell<Value>> = Rc::new(RefCell::new(Value::Undef));
                let mut rtn = Value::Undef; // (and $wrn, below)
                let opts = dy_file.at(2);
                if !fname.is_undef() && pl::call(it, "main::SlurpFile", vec![fname.clone(), Value::ScalarRef(buff.clone())])?.truthy() {
                    if get(it, "verbose").truthy() {
                        print_vout(it, &[cat(&[s("Reading "), dy_file.at(0), s(" from "), fname.clone(), s("\n")])])?;
                    }
                    pl::at(3257);
                    let mut args = vec![dy_file.at(0), buff.borrow().clone()];
                    args.extend(hash_pairs(&pl::hderef(&opts)?));
                    let r = pl::method_list(it, &et, "SetNewValue", args)?;
                    rtn = arg(&r, 0);
                    let wrn = arg(&r, 1);
                    if wrn.truthy() {
                        warn_(it, pl::concat(&wrn, &s("\n")))?;
                    }
                }
                // remove this tag if we couldn't set it properly
                if !rtn.truthy() {
                    meth_void(it, &et, "SetNewValue", vec![dy_file.at(0), Value::Undef, s("Replace"), n(2), s("ProtectSaved"), opts.get("ProtectSaved")])?;
                }
                continue;
            } else if is_ref_type(&dy_file, "SCALAR") {
                // set new values from CSV or JSON database
                set(it, "evalWarning", Value::Undef);
                let ew = super::lex_scalar(it, "evalWarning");
                let handler = Value::Code(Rc::new(Code::Native(
                    Rc::from("main::__ANON__"),
                    Rc::new(move |_it: &mut Interp, a: Vec<Value>, _w: Want| {
                        let v = arg(&a, 0);
                        *ew.borrow_mut() = v.clone();
                        ret(v)
                    }),
                )));
                let ok = pl::local_sig_warn(it, handler, |it| set_from_database(it, &et, &file))?;
                if !ok {
                    return ret(n(0));
                }
            }
        }
    }
    let verbose = get(it, "verbose");
    if is_stdout {
        // write to STDOUT
        outfile = handle_op(it, r"\*STDOUT", &[])?.0;
        if !get(it, "binaryStdout").truthy() {
            handle_op(it, "binmode(STDOUT)", &[])?;
            set(it, "binaryStdout", n(1));
        }
    } else {
        // get name of hard link if we are creating one
        hard_link = pl::method(it, &et, "GetNewValues", vec![s("HardLink")])?;
        sym_link = pl::method(it, &et, "GetNewValues", vec![s("SymLink")])?;
        test_name = pl::method(it, &et, "GetNewValues", vec![s("TestName")])?;
        if !hard_link.is_undef() {
            hard_link = pl::call(it, "main::FilenameSPrintf", vec![hard_link, orig.clone()])?;
        }
        if !sym_link.is_undef() {
            sym_link = pl::call(it, "main::FilenameSPrintf", vec![sym_link, orig.clone()])?;
        }
        // determine what our output file name should be
        let mut new_file_name = pl::method(it, &et, "GetNewValues", vec![s("FileName")])?;
        let mut new_dir = pl::method(it, &et, "GetNewValues", vec![s("Directory")])?;
        if !new_file_name.is_undef() && pl::length(&new_file_name).to_i64() == 0 {
            call_void(it, "main::Warning", vec![et.clone(), cat(&[s("New file name is empty - "), infile.clone()])])?;
            new_file_name = Value::Undef;
        }
        if !test_name.is_undef() {
            let mut err = Value::Undef;
            if !new_file_name.is_undef() || !new_dir.is_undef() {
                err = s("You shouldn't write FileName or Directory with TestFile");
            }
            if !outfile.is_undef() {
                err = s("The -o option shouldn't be used with TestFile");
            }
            if err.truthy() {
                warn_(it, cat(&[s("Error: "), err, s(" - "), infile.clone(), s("\n")]))?;
                efile(it, &infile, None)?;
                inc(it, "countBadWr");
                return ret(n(0));
            }
            test_name = pl::call(it, "main::FilenameSPrintf", vec![test_name, orig.clone()])?;
            if !pl::str_eq(&file, "") {
                test_name = pl::call(it, "Image::ExifTool::GetNewFileName", vec![file.clone(), test_name])?;
            }
        }
        if !new_file_name.is_undef() || !new_dir.is_undef() || (do_set_file_name.truthy() && !outfile.is_undef()) {
            if new_file_name.truthy() {
                new_file_name = pl::call(it, "main::FilenameSPrintf", vec![new_file_name, orig.clone()])?;
                if !outfile.is_undef() {
                    if !pl::str_eq(&file, "") {
                        outfile = pl::call(it, "Image::ExifTool::GetNewFileName", vec![file.clone(), outfile])?;
                    }
                    outfile = pl::call(it, "Image::ExifTool::GetNewFileName", vec![outfile, new_file_name])?;
                } else if !pl::str_eq(&file, "") {
                    outfile = pl::call(it, "Image::ExifTool::GetNewFileName", vec![file.clone(), new_file_name])?;
                }
            }
            if new_dir.truthy() {
                new_dir = pl::call(it, "main::FilenameSPrintf", vec![new_dir, orig.clone()])?;
                let base = if !outfile.is_undef() { outfile.clone() } else { file.clone() };
                outfile = pl::call(it, "Image::ExifTool::GetNewFileName", vec![base, new_dir])?;
            }
            outfile = pl::call(it, "main::NextUnusedFilename", vec![outfile, infile.clone()])?;
            if pl::method(it, &et, "Exists", vec![outfile.clone(), n(1)])?.truthy() {
                if infile.to_bytes() == outfile.to_bytes() {
                    outfile = Value::Undef; // not changing the file name after all
                // (allow for case-insensitive filesystems)
                } else if pl::method(it, &et, "IsSameFile", vec![infile.clone(), outfile.clone()])?.truthy() {
                    same_file = outfile.clone(); // same file, but the name has a different case
                } else {
                    warn_(it, cat(&[s("Error: '"), outfile.clone(), s("' already exists - "), infile.clone(), s("\n")]))?;
                    efile(it, &infile, None)?;
                    inc(it, "countBadWr");
                    return ret(n(0));
                }
            }
        }
        if !outfile.is_undef() {
            if !verbose.is_undef() {
                print_vout(it, &[cat(&[s("'"), infile.clone(), s("' --> '"), outfile.clone(), s("'\n")])])?;
            }
            // create output directory if necessary
            call_void(it, "main::CreateDirectory", vec![outfile.clone()])?;
            // set temporary file (automatically erased on abnormal exit)
            if !get(it, "outOpt").is_undef() {
                set(it, "tmpFile", outfile.clone());
            }
        }
        if get(it, "tmpFile").is_undef() {
            // count the number of tags and pseudo-tags we are writing
            let r = pl::method_list(it, &et, "CountNewValues", vec![])?;
            let (mut num_set, num_pseudo) = (arg(&r, 0), arg(&r, 1));
            if !pl::num_eq(&num_set, &num_pseudo) && pl::method(it, &et, "IsDirectory", vec![file.clone()])?.truthy() {
                if !verbose.is_undef() {
                    print_vout(it, &[cat(&[s("Can't write real tags to a directory - "), infile.clone(), s("\n")])])?;
                }
                num_set = num_pseudo.clone();
            }
            if pl::method(it, &et, "Exists", vec![file.clone()])?.truthy() {
                if !num_set.truthy() {
                    // no need to write if no tags set
                    if !verbose.is_undef() {
                        print_vout(it, &[cat(&[s("Nothing changed in "), file.clone(), s("\n")])])?;
                    }
                    efile(it, &infile, Some(1))?;
                    inc(it, "countSameWr");
                    return ret(n(1));
                }
            } else if pl::call(it, "Image::ExifTool::CanCreate", vec![file.clone()])?.truthy() {
                if pl::num_eq(&num_set, &num_pseudo) {
                    // no need to write if no real tags
                    warn_(it, cat(&[s("Error: Nothing to write - "), file.clone(), s("\n")]))?;
                    efile(it, &infile, Some(1))?;
                    inc(it, "countBadWr");
                    return ret(n(0));
                }
                if outfile.is_undef() {
                    // create file from scratch
                    outfile = file.clone();
                    file = s("");
                }
            } else {
                // file doesn't exist, and we can't create it
                warn_(it, cat(&[s("Error: File not found - "), file.clone(), s("\n")]))?;
                efile(it, &infile, None)?;
                call_void(it, "main::FileNotFound", vec![file.clone()])?;
                inc(it, "countBadWr");
                return ret(n(0));
            }
            // quickly rename file and/or set file date if this is all we are doing
            if pl::num_eq(&num_set, &num_pseudo) {
                let (mut r0, mut r1, mut r2, mut r3) = (n(0), n(0), n(0), n(0));
                if !outfile.is_undef() {
                    r0 = pl::method(it, &et, "SetFileName", vec![file.clone(), outfile.clone()])?;
                    if pl::gt(&r0, &n(0)) {
                        file = et.get("NewName"); // continue with new name if changed
                    }
                }
                if !pl::lt(&r0, &n(0)) {
                    r1 = pl::method(it, &et, "SetFileModifyDate", vec![file.clone(), Value::Undef, s("FileCreateDate")])?;
                    r2 = pl::method(it, &et, "SetFileModifyDate", vec![file.clone()])?;
                    r3 = pl::method(it, &et, "SetSystemTags", vec![file.clone()])?;
                }
                // (perl warns here if one of them is undef)
                let z = n(0);
                if pl::gt(&r0, &z) || pl::gt(&r1, &z) || pl::gt(&r2, &z) || pl::gt(&r3, &z) {
                    efile(it, &infile, Some(3))?;
                    inc(it, "countGoodWr");
                } else if pl::lt(&r0, &z) || pl::lt(&r1, &z) || pl::lt(&r2, &z) || pl::lt(&r3, &z) {
                    efile(it, &infile, None)?;
                    inc(it, "countBadWr");
                    return ret(n(0));
                } else {
                    efile(it, &infile, Some(1))?;
                    inc(it, "countSameWr");
                }
                if !hard_link.is_undef() || !sym_link.is_undef() || !test_name.is_undef() {
                    call_void(it, "main::DoHardLink", vec![et.clone(), file.clone(), hard_link, sym_link, test_name])?;
                }
                return ret(n(1));
            }
            if outfile.is_undef() || !same_file.is_undef() {
                // write to a truly temporary file
                outfile = pl::concat(&file, &s("_exiftool_tmp"));
                if pl::method(it, &et, "Exists", vec![outfile.clone()])?.truthy() {
                    warn_(it, cat(&[s("Error: Temporary file already exists: "), outfile.clone(), s("\n")]))?;
                    efile(it, &infile, None)?;
                    inc(it, "countBadWr");
                    return ret(n(0));
                }
                is_temporary = true;
            }
            // new output file is temporary until we know it has been written properly
            set(it, "tmpFile", outfile.clone());
        }
    }
    // rewrite the file
    let mut wargs = pl::call_list(it, "main::Infile", vec![file.clone()])?;
    wargs.push(outfile.clone());
    wargs.push(out_type.clone());
    let success = pl::method(it, &et, "WriteInfo", wargs)?;

    // create hard link if specified
    if success.truthy() && (!hard_link.is_undef() || !sym_link.is_undef() || !test_name.is_undef()) {
        let src = if !outfile.is_undef() { outfile.clone() } else { file.clone() };
        call_void(it, "main::DoHardLink", vec![et.clone(), src, hard_link, sym_link, test_name])?;
    }

    // get file time if preserving it
    let (mut a_time, mut m_time, mut c_time, mut do_preserve) = (Value::Undef, Value::Undef, Value::Undef, Value::Undef);
    if !pl::str_eq(&file, "") {
        do_preserve = get(it, "preserveTime");
    }
    if do_preserve.truthy() && success.truthy() {
        let r = pl::method_list(it, &et, "GetFileTime", vec![file.clone()])?;
        (a_time, m_time, c_time) = (arg(&r, 0), arg(&r, 1), arg(&r, 2));
        // don't override date/time values written by the user
        let written = et.get("WRITTEN");
        if written.get("FileCreateDate").truthy() {
            c_time = Value::Undef;
        }
        if written.get("FileModifyDate").truthy() || pl::num_eq(&do_preserve, &n(2)) {
            if !c_time.is_undef() {
                a_time = Value::Undef; // only preserve FileCreateDate
                m_time = Value::Undef;
            } else {
                do_preserve = Value::Undef; // (nothing to preserve)
            }
        }
    }

    let overwrite_orig = get(it, "overwriteOrig");
    // (perl warns here if $success is undef)
    if pl::num_eq(&success, &n(1)) {
        // preserve the original file times
        if !get(it, "tmpFile").is_undef() {
            if pl::method(it, &et, "Exists", vec![file.clone()])?.truthy() {
                if do_preserve.truthy() {
                    let tmp = get(it, "tmpFile");
                    meth_void(it, &et, "SetFileTime", vec![tmp, a_time.clone(), m_time.clone(), c_time.clone()])?;
                }
                if is_temporary {
                    // preserve original file attributes if possible
                    meth_void(it, &et, "CopyFileAttrs", vec![file.clone(), outfile.clone()])?;
                    // move original out of the way
                    let original = pl::concat(&file, &s("_original"));
                    if !overwrite_orig.truthy() && !pl::method(it, &et, "Exists", vec![original.clone()])?.truthy() {
                        // rename the file and check again to be sure the file doesn't exist
                        // (in case, say, the filesystem truncated the file extension)
                        if !pl::method(it, &et, "Rename", vec![file.clone(), original])?.truthy()
                            || pl::method(it, &et, "Exists", vec![file.clone()])?.truthy()
                        {
                            call_void(it, "main::Error", vec![cat(&[s("Error renaming "), file.clone(), s("\n")])])?;
                            return ret(n(0));
                        }
                    }
                    let dst_file = if !same_file.is_undef() { same_file.clone() } else { file.clone() };
                    if pl::gt(&overwrite_orig, &n(1)) {
                        // copy temporary file over top of original to preserve attributes
                        let mut err = false;
                        let mut buff = Value::Undef;
                        let new_file = get(it, "tmpFile");
                        let opened = handle_op(it, r"$et->Open(\*NEW_FILE, $f)", &[("et", et.clone()), ("f", new_file.clone())])?.0;
                        if !opened.truthy() {
                            call_void(it, "main::Error", vec![cat(&[s("Error opening "), new_file.clone(), s("\n")])])?;
                            return ret(n(0));
                        }
                        handle_op(it, "binmode(NEW_FILE)", &[])?;

                        //..........................................................
                        // temporarily disable CTRL-C during this critical operation
                        set(it, "critical", n(1));
                        set(it, "tmpFile", Value::Undef); // handle deletion of temporary file ourself
                        let opened = handle_op(it, r"$et->Open(\*ORIG_FILE, $f, '+<')", &[("et", et.clone()), ("f", file.clone())])?.0;
                        if opened.truthy() {
                            handle_op(it, "binmode(ORIG_FILE)", &[])?;
                            loop {
                                let (r, v) = handle_op(it, "read(NEW_FILE, $buff, 65536)", &[("buff", buff.clone())])?;
                                buff = arg(&v, 0);
                                if !r.truthy() {
                                    break;
                                }
                                if !handle_op(it, "print ORIG_FILE $buff", &[("buff", buff.clone())])?.0.truthy() {
                                    err = true;
                                }
                            }
                            handle_op(it, "close(NEW_FILE)", &[])?;
                            // Handle files being shorter than the original
                            let r = match pl::eval_block(it, |it| handle_op(it, "truncate(ORIG_FILE, tell(ORIG_FILE))", &[]))? {
                                Some((v, _)) => v,
                                None => Value::Undef,
                            };
                            if !r.truthy() {
                                err = true;
                            }
                            if !handle_op(it, "close(ORIG_FILE)", &[])?.0.truthy() {
                                err = true;
                            }
                            if err {
                                warn_(it, cat(&[s("Couldn't overwrite in place - "), file.clone(), s("\n")]))?;
                                if !(pl::method(it, &et, "Rename", vec![new_file.clone(), file.clone()])?.truthy()
                                    || (pl::method(it, &et, "Unlink", vec![file.clone()])?.truthy()
                                        && pl::method(it, &et, "Rename", vec![new_file.clone(), file.clone()])?.truthy()))
                                {
                                    call_void(it, "main::Error", vec![cat(&[s("Error renaming "), new_file.clone(), s(" to "), file.clone(), s("\n")])])?;
                                    set(it, "critical", Value::Undef);
                                    if get(it, "interrupted").truthy() {
                                        call_void(it, "main::SigInt", vec![])?;
                                    }
                                    return ret(n(0));
                                }
                            } else {
                                meth_void(it, &et, "SetFileModifyDate", vec![file.clone(), c_time.clone(), s("FileCreateDate"), n(1)])?;
                                meth_void(it, &et, "SetFileModifyDate", vec![file.clone(), m_time.clone(), s("FileModifyDate"), n(1)])?;
                                meth_void(it, &et, "Unlink", vec![new_file.clone()])?;
                                if do_preserve.truthy() {
                                    meth_void(it, &et, "SetFileTime", vec![file.clone(), a_time.clone(), m_time.clone(), c_time.clone()])?;
                                    // save time to set it later again to patch OS X 10.6 bug
                                    super::lex_hash(it, "preserveTime").set(file.to_bytes(), Value::new_array(vec![a_time.clone(), m_time.clone(), c_time.clone()]));
                                }
                            }
                            efile(it, &infile, Some(3))?;
                            inc(it, "countGoodWr");
                        } else {
                            handle_op(it, "close(NEW_FILE)", &[])?;
                            warn_(it, cat(&[s("Error opening "), file.clone(), s(" for writing\n")]))?;
                            efile(it, &infile, None)?;
                            meth_void(it, &et, "Unlink", vec![new_file.clone()])?;
                            inc(it, "countBadWr");
                        }
                        set(it, "critical", Value::Undef); // end critical section
                        if get(it, "interrupted").truthy() {
                            call_void(it, "main::SigInt", vec![])?; // issue delayed SIGINT if necessary
                        }
                        //..........................................................

                    // simply rename temporary file to replace original
                    // (if we didn't already rename it to add "_original")
                    } else if {
                        let tmp = get(it, "tmpFile");
                        pl::method(it, &et, "Rename", vec![tmp, dst_file.clone()])?.truthy()
                    } {
                        efile(it, &infile, Some(3))?;
                        inc(it, "countGoodWr");
                    } else {
                        let new_file = get(it, "tmpFile");
                        set(it, "tmpFile", Value::Undef); // (avoid deleting file if we get interrupted)
                        // unlink may fail if already renamed or no permission
                        if !pl::method(it, &et, "Unlink", vec![file.clone()])?.truthy() {
                            warn_(it, cat(&[s("Error renaming temporary file to "), dst_file.clone(), s("\n")]))?;
                            efile(it, &infile, None)?;
                            meth_void(it, &et, "Unlink", vec![new_file])?;
                            inc(it, "countBadWr");
                        // try renaming again now that the target has been deleted
                        } else if !pl::method(it, &et, "Rename", vec![new_file, dst_file.clone()])?.truthy() {
                            warn_(it, cat(&[s("Error renaming temporary file to "), dst_file.clone(), s("\n")]))?;
                            efile(it, &infile, None)?;
                            // (don't delete tmp file now because it is all we have left)
                            inc(it, "countBadWr");
                        } else {
                            efile(it, &infile, Some(3))?;
                            inc(it, "countGoodWr");
                        }
                    }
                } else if overwrite_orig.truthy() {
                    // erase original file
                    efile(it, &infile, Some(3))?;
                    if !pl::method(it, &et, "Unlink", vec![file.clone()])?.truthy() {
                        warn_(it, cat(&[s("Error erasing original "), file.clone(), s("\n")]))?;
                    }
                    inc(it, "countGoodWr");
                } else {
                    efile(it, &infile, Some(4))?;
                    inc(it, "countGoodCr");
                }
            } else {
                // this file was created from scratch, not edited
                efile(it, &infile, Some(4))?;
                inc(it, "countGoodCr");
            }
        } else {
            efile(it, &infile, Some(3))?;
            inc(it, "countGoodWr");
        }
    } else if success.truthy() {
        efile(it, &infile, Some(1))?;
        if is_temporary {
            // just erase the temporary file since no changes were made
            let tmp = get(it, "tmpFile");
            meth_void(it, &et, "Unlink", vec![tmp])?;
            inc(it, "countSameWr");
        } else {
            if do_preserve.truthy() {
                meth_void(it, &et, "SetFileTime", vec![outfile.clone(), a_time, m_time, c_time])?;
            }
            if overwrite_orig.truthy() && !pl::method(it, &et, "Unlink", vec![file.clone()])?.truthy() {
                warn_(it, cat(&[s("Error erasing original "), file.clone(), s("\n")]))?;
            }
            inc(it, "countCopyWr");
        }
        if !verbose.is_undef() {
            print_vout(it, &[cat(&[s("Nothing changed in "), file.clone(), s("\n")])])?;
        }
    } else {
        efile(it, &infile, None)?;
        if !get(it, "tmpFile").is_undef() {
            let tmp = get(it, "tmpFile");
            meth_void(it, &et, "Unlink", vec![tmp])?;
        }
        inc(it, "countBadWr");
    }
    set(it, "tmpFile", Value::Undef);
    ret(success)
}

/// The body of SetImageInfo's `elsif (ref $dyFile eq 'SCALAR')` block from
/// `my $old = $et->Options('Charset');` on (script lines 3268-3365), run
/// under its `local $SIG{'__WARN__'}`: set new values from a CSV or JSON
/// database. Returns false for the block's `return 0`.
fn set_from_database(it: &mut Interp, et: &Value, file: &Value) -> ER<bool> {
    let (mut found, mut csv_et_prt, mut csv_et_val) = (false, Value::Undef, Value::Undef);
    let csv = get(it, "csv");
    let database = super::lex_hash(it, "database");
    let tags = super::lex_array(it, "tags");
    let csv_exclude = super::lex_array(it, "csvExclude");
    // force UTF-8 if the database was JSON
    let old = pl::method(it, et, "Options", vec![s("Charset")])?;
    if pl::str_eq(&csv, "JSON") {
        meth_void(it, et, "Options", vec![s("Charset"), s("UTF8")])?;
    }
    // read tags for SourceFile '*' plus the specific file
    let mut fs = vec![s("*")];
    fs.extend(pl::call_list(it, "main::MyConvertFileName", vec![et.clone(), file.clone()])?);
    let skip = re(r"^([-_0-9A-Z]+:)*(SourceFile|Directory|FileName)$", "i");
    for f in fs {
        let mut csv_info = database.get(f.to_bytes());
        if !csv_info.truthy() {
            if pl::str_eq(&f, "*") {
                continue;
            }
            // check absolute path and case-insensitive name
            // (punt on using ConvertFileName here, so $absPath may be a mix of encodings)
            let abs_path = pl::call(it, "main::AbsPath", vec![f.clone()])?;
            if !abs_path.is_undef() && database.get(abs_path.to_bytes()).truthy() {
                csv_info = database.get(abs_path.to_bytes());
            } else if database.get(pl::lc(&f).to_bytes()).truthy() {
                csv_info = database.get(pl::lc(&f).to_bytes());
            } else if !abs_path.is_undef() && database.get(pl::lc(&abs_path).to_bytes()).truthy() {
                csv_info = database.get(pl::lc(&abs_path).to_bytes());
            } else {
                continue;
            }
        }
        found = true;
        if get(it, "verbose").truthy() {
            print_vout(it, &[cat(&[s("Setting new values from "), csv.clone(), s(" database\n")])])?;
            if !tags.is_empty() {
                print_vout(it, &[s("Including tags: "), pl::join(" ", &tags.items()), s("\n")])?;
            }
            if !csv_exclude.is_empty() {
                print_vout(it, &[s("Excluding tags: "), pl::join(" ", &csv_exclude.items()), s("\n")])?;
            }
        }
        if !tags.is_empty() {
            // prepare a dummy ExifTool object to hold appropriate tags from the database
            if !csv_et_prt.truthy() {
                csv_et_prt = pl::method(it, &s("Image::ExifTool"), "new", vec![])?;
            }
            for csv_tag in pl::call_list(it, "Image::ExifTool::OrderedKeys", vec![csv_info.clone()])? {
                if skip.is_match(&csv_tag) {
                    continue;
                }
                let mut grps = pl::split(&re(":", ""), &csv_tag, 0);
                let mut name = grps.pop().unwrap_or_default();
                while grps.len() < 2 {
                    grps.insert(0, s("All"));
                }
                let val = csv_info.get(csv_tag.to_bytes());
                if re("#$", "").subst(&mut name, false, |_| Vec::new()) > 0 {
                    // handle ValueConv tags separately
                    if !csv_et_val.truthy() {
                        csv_et_val = pl::method(it, &s("Image::ExifTool"), "new", vec![])?;
                    }
                    let mut a = vec![name, val];
                    a.extend(grps);
                    meth_void(it, &csv_et_val, "FoundTag", a)?;
                } else {
                    let mut a = vec![name, val];
                    a.extend(grps);
                    meth_void(it, &csv_et_prt, "FoundTag", a)?;
                }
            }
            continue;
        }
        let mut excl_tags = csv_exclude.items();
        for x in excl_tags.iter_mut() {
            pl::tr(x, "-0-9a-zA-Z_:#?*", "", "dc"); // remove illegal characters
            re("(^|:)(all:)+", "i").subst(x, true, |c| cap(c, 1).to_bytes().to_vec()); // remove 'all' group names
            re("(^|:)all(#?)$", "i").subst(x, false, |c| [&cap(c, 1).to_bytes()[..], b"*", &cap(c, 2).to_bytes()[..]].concat()); // convert 'all' tag name to '*'
            pl::tr(x, "?", ".", ""); // convert wildcards for regex
            re(r"\*", "").subst(x, true, |_| b".*".to_vec());
        }
        // run through tags in database order
        for csv_tag in pl::call_list(it, "Image::ExifTool::OrderedKeys", vec![csv_info.clone()])? {
            // don't write SourceFile, Directory or FileName
            if skip.is_match(&csv_tag) {
                continue;
            }
            if !excl_tags.is_empty() {
                let mut excluded = false;
                'excl_match: for excl_tag in &excl_tags {
                    if re(":", "").is_match(excl_tag) {
                        if !re(":", "").is_match(&csv_tag) {
                            continue; // db entry must also specify group
                        }
                        let mut csv_grps = pl::split(&re(":", ""), &csv_tag, 0);
                        let mut excl_grps = pl::split(&re(":", ""), excl_tag, 0);
                        let excl_name = excl_grps.pop().unwrap_or_default();
                        let last = csv_grps.pop().unwrap_or_default();
                        if !pl::re_dyn(&[b"^", &excl_name.to_bytes()[..], b"$"].concat(), "i")?.is_match(&last) {
                            continue; // tag name must match
                        }
                        for excl_grp in &excl_grps {
                            // each specified group name must match db entry
                            let g = pl::re_dyn(&[b"^", &excl_grp.to_bytes()[..], b"$"].concat(), "i")?;
                            if !csv_grps.iter().any(|x| g.is_match(x)) {
                                continue 'excl_match;
                            }
                        }
                        excluded = true;
                        break;
                    }
                    // no group specified, so match by tag name only
                    if pl::re_dyn(&[b"^([-_0-9A-Z]+:)*", &excl_tag.to_bytes()[..], b"$"].concat(), "i")?.is_match(&csv_tag) {
                        excluded = true;
                        break;
                    }
                }
                if excluded {
                    continue;
                }
            }
            let (db_add, db_save_count) = (get(it, "dbAdd"), get(it, "dbSaveCount"));
            let r = pl::method_list(
                it,
                et,
                "SetNewValue",
                vec![csv_tag.clone(), csv_info.get(csv_tag.to_bytes()), s("Protected"), n(1), s("AddValue"), db_add, s("ProtectSaved"), db_save_count],
            )?;
            let wrn = arg(&r, 1);
            if get(it, "verbose").truthy() && wrn.truthy() {
                warn_(it, pl::concat(&wrn, &s("\n")))?;
            }
        }
    }
    // set specified tags now
    if csv_et_prt.truthy() {
        let excl: Vec<Value> = csv_exclude.items().iter().map(|x| pl::concat(&s("-"), x)).collect(); // add back leading dashes
        let opts = Value::new_hash();
        opts.set("AddValue", get(it, "dbAdd"));
        opts.set("Replace", n(0));
        let mut a = vec![csv_et_prt.clone(), opts.clone()];
        a.extend(tags.items());
        a.extend(excl.iter().cloned());
        meth_void(it, et, "SetNewValuesFromFile", a)?;
        if csv_et_val.truthy() {
            opts.set("Type", s("ValueConv"));
            let mut a = vec![csv_et_val.clone(), opts.clone()];
            a.extend(tags.items());
            a.extend(excl);
            meth_void(it, et, "SetNewValuesFromFile", a)?;
        }
    }
    if pl::str_eq(&csv, "JSON") {
        meth_void(it, et, "Options", vec![s("Charset"), old])?;
    }
    if !found {
        warn_(it, cat(&[s("No SourceFile '"), file.clone(), s("' in imported "), csv.clone(), s(" database\n")]))?;
        let abs_path = pl::call(it, "main::AbsPath", vec![file.clone()])?;
        if !abs_path.is_undef() && abs_path.to_bytes() != file.to_bytes() {
            warn_(it, cat(&[s("(full path: '"), abs_path, s("')\n")]))?;
        }
        return Ok(false);
    }
    Ok(true)
}

pub fn register(it: &mut Interp) {
    super::define(it, "main::SetImageInfo", set_image_info);
}
