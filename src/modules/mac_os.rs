//! Image::ExifTool::MacOS (Image/ExifTool/MacOS.pm), ported sub by sub (see docs/port/PORTING.md).
//!
//! Read/write MacOS system tags. The subs that run the MacOS utilities
//! (mdls, xattr, stat, setfile, tag, osascript) do it as the Perl does,
//! through perl's backticks and system(); they are only called on a Mac.

// (the Perl assigns values it does not read again; the port keeps them)
#![allow(unused_assignments)]

use crate::interp::eval::{Cell, ER};
use crate::interp::{Flow, Interp, Scope, Want};
use crate::modules::random_access::{cell, raf_read_cell, substr};
use crate::perl::Value;
use crate::pl::{self, arg, cap, n, re, ret};
use crate::runtime;

/// `$raf->METHOD(...)` on an undefined `$raf` dies as perl does, at the
/// statement's location (the Read helpers run the call as an eval string,
/// which has a location of its own).
fn check_invocant(raf: &Value, method: &str) -> ER<()> {
    if raf.is_undef() {
        return crate::interp::eval::die(format!("Can't call method \"{method}\" on an undefined value"));
    }
    Ok(())
}

/// `Image::ExifTool::NAME(ARGS)` (the functions imported with :DataAccess :Utils).
fn call(it: &mut Interp, name: &str, args: Vec<Value>) -> ER<Value> {
    pl::call(it, &format!("Image::ExifTool::{name}"), args)
}

/// `\$buff` of a `my` variable kept in a cell.
fn rf(c: &Cell) -> Value {
    Value::ScalarRef(c.clone())
}

fn val(c: &Cell) -> Value {
    c.borrow().clone()
}

/// `$raf->Read($buff, $len)` into the `my $buff` cell.
fn read(it: &mut Interp, raf: &Value, buff: &Cell, len: Value) -> ER<Value> {
    raf_read_cell(it, raf, buff, len)
}

fn warn(it: &mut Interp, et: &Value, msg: Value) -> ER<()> {
    pl::method(it, et, "Warn", vec![msg])?;
    Ok(())
}

fn cat(parts: &[&Value]) -> Value {
    let mut v = Vec::new();
    for p in parts {
        v.extend_from_slice(&p.to_bytes());
    }
    Value::bytes(v)
}

fn handle_tag(it: &mut Interp, et: &Value, table: &Value, tag: Value, v: Value, opts: &[(&str, Value)]) -> ER<Value> {
    let mut args = vec![table.clone(), tag, v];
    for (k, x) in opts {
        args.push(pl::s(k));
        args.push(x.clone());
    }
    pl::method(it, et, "HandleTag", args)
}

/// Run a short Perl statement with `@args` and `$cmd` in scope, for the
/// builtins a native can't call (system, backticks, filehandle dups).
/// `$@` is left as it was; an error is a die.
fn perl_run(it: &mut Interp, code: &str, cmd: Value, args: Vec<Value>, want: Want) -> ER<Vec<Value>> {
    let scope = Scope::new(None);
    scope.scalar("cmd", cmd);
    scope.array("args", args);
    let saved = it.get_scalar("main::@");
    let r = it.eval_string(code.as_bytes(), "Image::ExifTool::MacOS", &scope, want);
    it.set_scalar("main::@", saved);
    r.map_err(Flow::Die)
}

/// `` `CMD` `` in list context, and `$?` after it.
fn backticks_list(it: &mut Interp, cmd: Value) -> ER<(Vec<Value>, Value)> {
    let r = perl_run(it, "my @r = `$cmd`; @r", cmd, vec![], Want::List)?;
    Ok((r, it.get_scalar("main::?")))
}

/// `` `CMD` `` in scalar context, and `$?` after it.
fn backticks(it: &mut Interp, cmd: Value) -> ER<(Value, Value)> {
    let r = perl_run(it, "my $r = `$cmd`; $r", cmd, vec![], Want::Scalar)?;
    Ok((r.into_iter().last().unwrap_or_default(), it.get_scalar("main::?")))
}

/// `chomp` (with the default `$/`)
fn chomp(v: &mut Value) {
    let b = v.to_bytes();
    if b.last() == Some(&b'\n') {
        *v = Value::bytes(&b[..b.len() - 1]);
    }
}

/// `($fn = $file) =~ s/([`"\$\\])/\\$1/g;   # escape necessary characters`
fn escape_fn(file: &Value) -> Value {
    let mut f = file.clone();
    re(r#"([`"\$\\])"#, "").subst(&mut f, true, |c| [&b"\\"[..], &cap(c, 1).to_bytes()[..]].concat());
    f
}

fn md_item_local_time_impl(it: &mut Interp, mut v: Value) -> ER<Value> {
    pl::tr(&mut v, "-", ":", "");
    re(r" ?([-+]\d{2}):?(\d{2})", "").subst(&mut v, false, |c| [&cap(c, 1).to_bytes()[..], b":", &cap(c, 2).to_bytes()[..]].concat());
    // convert from UTC to local time
    if re(r"\+00:00$", "").is_match(&v) {
        let time = call(it, "GetUnixTime", vec![v.clone()])?;
        if time.truthy() {
            v = call(it, "ConvertUnixTime", vec![time, n(1)])?;
        }
    }
    Ok(v)
}

/// Convert OS MDItem time string to standard EXIF-formatted local time
/// Inputs: 0) time string (eg. "2017-02-21 17:21:43 +0000")
/// Returns: EXIF-formatted local time string with timezone
pub fn md_item_local_time(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    ret(md_item_local_time_impl(it, arg(&a, 0))?)
}

fn system_impl(it: &mut Interp, args: Vec<Value>) -> ER<Value> {
    let code = r#"my ($oldout, $olderr);
open($oldout, ">&STDOUT");
open($olderr, ">&STDERR");
open(STDOUT, '>', '/dev/null');
open(STDERR, '>', '/dev/null');
my $result = system(@args);
open(STDOUT, ">&", $oldout);
open(STDERR, ">&", $olderr);
$result"#;
    Ok(perl_run(it, code, Value::Undef, args, Want::Scalar)?.into_iter().last().unwrap_or_default())
}

/// Call system command, redirecting all I/O to /dev/null
/// Inputs: system arguments
/// Returns: system return code
pub fn system(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    ret(system_impl(it, a)?)
}

/// Set MacOS MDItem and XAttr tags from new tag values
/// Inputs: 0) ExifTool ref, 1) file name, 2) list of tags to set
/// Returns: 1=something was set OK, 0=didn't try, -1=error (and warning set)
/// Notes: There may be errors even if 1 is returned
pub fn set_mac_os_tags(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, file, set_tags) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    let mut result = n(0);
    let del_x_attr = runtime::lex_hash(it, "Image::ExifTool::MacOS", "delXAttr");

    // (foreach autovivifies an undefined $setTags)
    let set_tags_items = if set_tags.is_undef() {
        Vec::new()
    } else {
        pl::at(395);
        pl::aderef(&set_tags)?
    };
    for tag in set_tags_items {
        let mut cmd: Vec<Value> = Vec::new();
        let (mut attr, mut err, mut silent_err) = (Value::Undef, Value::Undef, Value::Undef);
        let nv_ref = pl::sref(Value::Undef);
        let mut v = pl::method(it, &et, "GetNewValue", vec![tag.clone(), nv_ref.clone()])?;
        let nv_hash = nv_ref.deref();
        if !nv_hash.truthy() {
            continue;
        }
        let overwrite = pl::method(it, &et, "IsOverwriting", vec![nv_hash.clone()])?;
        if !nv_hash.get("TagInfo").get("List").truthy() {
            if !overwrite.truthy() {
                continue;
            }
            if pl::lt(&overwrite, &n(0)) {
                let operation = if nv_hash.get("Shift").truthy() { "Shifting" } else { "Conditional replacement" };
                warn(it, &et, cat(&[&pl::s(operation), &pl::s(" of MacOS "), &tag, &pl::s(" not yet supported")]))?;
                continue;
            }
        }
        if pl::str_eq(&tag, "MDItemFSCreationDate") || pl::str_eq(&tag, "FileCreateDate") {
            // convert to local time if value has a time zone
            if re(r"[-+Z]", "").is_match(&v) {
                let time = call(it, "GetUnixTime", vec![v.clone(), n(1)])?;
                if time.truthy() {
                    v = call(it, "ConvertUnixTime", vec![time, n(1)])?;
                }
                re(r"[-+].*", "").subst(&mut v, false, |_| Vec::new()); // remove time zone
            }
            // reformat for setfile
            re(r"(\d{4}):(\d{2}):(\d{2})", "").subst(&mut v, false, |c| [&cap(c, 2).to_bytes()[..], b"/", &cap(c, 3).to_bytes()[..], b"/", &cap(c, 1).to_bytes()[..]].concat());
            cmd.extend([pl::s("/usr/bin/setfile"), pl::s("-d"), v.clone(), file.clone()]);
        } else if pl::str_eq(&tag, "MDItemUserTags") {
            // (tested with "tag" version 0.9.0)
            let vals = pl::method_list(it, &et, "GetNewValue", vec![nv_hash.clone()])?;
            let dels: Vec<Value>;
            if pl::lt(&overwrite, &n(0)) && {
                pl::at(420);
                dels = pl::aderef(&nv_hash.get("DelValue"))?;
                !dels.is_empty()
            } {
                // delete specified tags
                let del = pl::join(",", &dels);
                err = system_impl(it, vec![pl::s("/usr/local/bin/tag"), pl::s("-r"), del.clone(), file.clone()])?;
                if !err.truthy() {
                    pl::method(it, &et, "VerboseValue", vec![cat(&[&pl::s("- "), &tag]), del])?;
                    result = n(1);
                    if !vals.is_empty() {
                        err = Value::Undef; // more to do if there are tags to add
                    }
                }
            }
            if err.is_undef() {
                // add new tags, or overwrite or delete existing tags
                let opt = if pl::gt(&overwrite, &n(0)) { "-s" } else { "-a" };
                v = if !vals.is_empty() { pl::join(",", &vals) } else { pl::s("") };
                cmd.extend([pl::s("/usr/local/bin/tag"), pl::s(opt), v.clone(), file.clone()]);
                if pl::gt(&overwrite, &n(0)) {
                    pl::method(it, &et, "VPrint", vec![n(1), cat(&[&pl::s("    - "), &tag, &pl::s(" = (all)\n")])])?;
                }
                if pl::str_eq(&v, "") {
                    v = Value::Undef;
                }
            }
        } else if del_x_attr.get(tag.to_bytes()).truthy() {
            cmd.extend([pl::s("/usr/bin/xattr"), pl::s("-d"), del_x_attr.get(tag.to_bytes()), file.clone()]);
            silent_err = n(256); // (will get this error if attribute doesn't exist)
        } else {
            let mut f = file.clone();
            re(r#"([\\"])"#, "").subst(&mut f, true, |c| [&b"\\"[..], &cap(c, 1).to_bytes()[..]].concat()); // escape backslashes and quotes for AppleScript
            let vv;
            if pl::str_eq(&tag, "MDItemFinderComment") {
                // (write finder comment using osascript instead of xattr
                // because it is more work to construct the necessary bplist)
                if v.is_undef() {
                    v = pl::s(""); // set to empty string instead of deleting
                }
                let mut e = pl::method(it, &et, "Encode", vec![v.clone(), pl::s("UTF8")])?;
                re(r#"([\\"])"#, "").subst(&mut e, true, |c| [&b"\\"[..], &cap(c, 1).to_bytes()[..]].concat());
                vv = e;
                attr = pl::s("comment");
            } else {
                // $tag eq 'MDItemFSLabel'
                vv = if v.truthy() { pl::sub(&n(8), &v) } else { n(0) }; // convert from label to label index (0 for no label)
                attr = pl::s("label index");
            }
            cmd.extend([
                pl::s("/usr/bin/osascript"),
                pl::s("-e"),
                cat(&[&pl::s("set fp to POSIX file \""), &f, &pl::s("\" as alias")]),
                pl::s("-e"),
                cat(&[&pl::s("tell application \"Finder\" to set "), &attr, &pl::s(" of file fp to \""), &vv, &pl::s("\"")]),
            ]);
        }
        if !cmd.is_empty() {
            err = system_impl(it, cmd.clone())?;
        }
        if !err.truthy() {
            if !v.is_undef() {
                pl::method(it, &et, "VerboseValue", vec![cat(&[&pl::s("+ "), &tag]), v.clone()])?;
            }
            result = n(1);
        } else if !silent_err.truthy() || !pl::num_eq(&err, &silent_err) {
            let c0 = cmd.first().cloned().unwrap_or_default();
            let mut c = if c0.truthy() { c0 } else { pl::s("tag") };
            re(r".*/", "").subst(&mut c, false, |_| Vec::new());
            warn(it, &et, cat(&[&pl::s("Error "), &err, &pl::s(" running \""), &c, &pl::s("\" to set "), &tag]))?;
            if !result.truthy() {
                result = n(-1);
            }
        }
    }
    ret(result)
}

/// Extract MacOS metadata item tags
/// Inputs: 0) ExifTool object ref, 1) file name
pub fn extract_md_item_tags(it: &mut Interp, a: Vec<Value>, want: Want) -> ER<Vec<Value>> {
    pl::local_topic(it, |it| {
        let (et, file) = (arg(&a, 0), arg(&a, 1));
        let (mut tag, mut v) = (Value::Undef, Value::Undef);

        let fname = escape_fn(&file); // escape necessary characters
        pl::method(it, &et, "VPrint", vec![n(0), pl::s("(running mdls)")])?;
        let cmd = cat(&[&pl::s("/usr/bin/mdls \""), &fname, &pl::s("\" 2> /dev/null")]);
        let (mdls, status) = backticks_list(it, cmd)?; // get MacOS metadata
        if status.truthy() || mdls.is_empty() {
            warn(it, &et, pl::s("Error running \"mdls\" to extract MDItem tags"))?;
            return pl::ret_nothing(want);
        }
        let tag_table_ptr = call(it, "GetTagTable", vec![pl::s("Image::ExifTool::MacOS::MDItem")])?;
        et.set("INDENT", cat(&[&et.get("INDENT"), &pl::s("| ")]));
        pl::method(it, &et, "VerboseDir", vec![pl::s("MDItem")])?;
        for mut l in mdls {
            chomp(&mut l);
            if pl::ref_of(&v) != "ARRAY" {
                let mut t = Value::Undef;
                if re(r"^k?(\w+)\s*= ", "").subst(&mut l, false, |c| {
                    t = cap(c, 1);
                    Vec::new()
                }) == 0
                {
                    continue;
                }
                tag = t;
                if pl::str_eq(&l, "(") {
                    v = Value::new_array(Vec::new()); // (start of a list)
                    continue;
                }
                if pl::str_eq(&l, "(null)") {
                    l = pl::s("");
                }
                if re(r#"^""#, "").subst(&mut l, false, |_| Vec::new()) > 0 {
                    re(r#""$"#, "").subst(&mut l, false, |_| Vec::new()); // remove quotes if they exist
                }
                v = l.clone();
            } else if pl::str_eq(&l, ")") {
                // (end of a list)
                l = v.at(0);
                if l.is_undef() {
                    continue;
                }
            } else {
                // add item to list
                re(r"^    ", "").subst(&mut l, false, |_| Vec::new()); // remove leading spaces
                re(r",$", "").subst(&mut l, false, |_| Vec::new()); // remove trailing comma
                if pl::str_eq(&l, "(null)") {
                    l = pl::s("");
                }
                if re(r#"^""#, "").subst(&mut l, false, |_| Vec::new()) > 0 {
                    re(r#""$"#, "").subst(&mut l, false, |_| Vec::new()); // remove quotes if they exist
                }
                re(r#"\\""#, "").subst(&mut l, true, |_| b"\"".to_vec()); // un-escape quotes
                re(r"\\\\", "").subst(&mut l, true, |_| b"\\".to_vec()); // un-escape backslashes
                l = pl::method(it, &et, "Decode", vec![l, pl::s("UTF8")])?;
                v.push(l);
                continue;
            }
            // add to Extra tags if not done already
            if !tag_table_ptr.get(tag.to_bytes()).truthy() {
                // check for a date/time format
                let tag_info = Value::new_hash();
                if re(r"^\d{4}-\d{2}-\d{2} \d{2}:\d{2}:\d{2}", "").is_match(&l) {
                    let g = Value::new_hash();
                    g.set("2", pl::s("Time"));
                    tag_info.set("Groups", g);
                    tag_info.set("ValueConv", Value::Code(it.sub_ref("Image::ExifTool::MacOS::MDItemLocalTime")));
                    tag_info.set("PrintConv", pl::s("$self->ConvertDateTime($val)"));
                }
                // change tags like "com_apple_mail_xxx" to "AppleMailXxx"
                let mut tmp = tag.clone();
                re(r"^com_", "").subst(&mut tmp, false, |_| Vec::new()); // remove leading "com_"
                re(r"_([a-z])", "").subst(&mut tmp, true, |c| pl::ucfirst(&cap(c, 1)).to_bytes().to_vec()); // use CamelCase
                tag_info.set("Name", call(it, "MakeTagName", vec![tmp])?);
                if pl::ref_of(&v) == "ARRAY" {
                    tag_info.set("List", n(1));
                }
                if re(r"Audio", "").is_match(&tag) {
                    tag_info.get_or_new_hash("Groups").set("2", pl::s("Audio"));
                }
                if re(r"(Copyright|Author)", "").is_match(&tag) {
                    tag_info.get_or_new_hash("Groups").set("2", pl::s("Author"));
                }
                pl::method(it, &et, "VPrint", vec![n(0), cat(&[&pl::s("  [adding "), &tag, &pl::s("]\n")])])?;
                call(it, "AddTagToTable", vec![tag_table_ptr.clone(), tag.clone(), tag_info])?;
            }
            if !v.is_ref() {
                v = pl::method(it, &et, "Decode", vec![v, pl::s("UTF8")])?;
            }
            handle_tag(it, &et, &tag_table_ptr, tag.clone(), v.clone(), &[])?;
            v = Value::Undef;
        }
        let mut indent = et.get("INDENT");
        let r = re(r"\| $", "").subst(&mut indent, false, |_| Vec::new());
        et.set("INDENT", indent);
        ret(if r > 0 { n(1) } else { pl::s("") })
    })
}

fn read_x_attr_value_impl(it: &mut Interp, et: &Value, tag_table_ptr: &Value, tag: &Value, v: Value) -> ER<Value> {
    let mut v = v;
    // add to our table if necessary
    if !tag_table_ptr.get(tag.to_bytes()).truthy() {
        let mut name;
        // generate tag name from attribute name
        if let Some(m) = re(r"^com\.apple\.(.*)$", "").m(tag) {
            name = cap(&m, 1);
            re(r"^metadata:_?k", "").subst(&mut name, false, |_| Vec::new());
            re(r"^metadata:(com_)?", "").subst(&mut name, false, |_| Vec::new());
        } else {
            name = tag.clone();
        }
        re(r"[.:_]([a-z])", "").subst(&mut name, true, |c| pl::uc(&cap(c, 1)).to_bytes().to_vec());
        name = cat(&[&pl::s("XAttr"), &pl::ucfirst(&name)]);
        let tag_info = Value::new_hash();
        tag_info.set("Name", name);
        if re(r"Date$", "").is_match(tag) {
            let g = Value::new_hash();
            g.set("2", pl::s("Time"));
            tag_info.set("Groups", g);
        }
        pl::method(it, et, "VPrint", vec![n(0), cat(&[&pl::s("  [adding "), tag, &pl::s("]\n")])])?;
        call(it, "AddTagToTable", vec![tag_table_ptr.clone(), tag.clone(), tag_info])?;
    }
    if re(r"^bplist0", "").is_match(&v) {
        let v_cell = cell(v.clone());
        let dir_info = Value::new_hash();
        dir_info.set("DataPt", rf(&v_cell));
        runtime::require(it, "Image::ExifTool::PLIST")?;
        if pl::call(it, "Image::ExifTool::PLIST::ProcessBinaryPLIST", vec![et.clone(), dir_info.clone(), tag_table_ptr.clone()])?.truthy() {
            if pl::ref_of(&dir_info.get("Value")) == "HASH" {
                return Ok(Value::Undef);
            }
            v = dir_info.get("Value");
        } else {
            let name = tag_table_ptr.get(tag.to_bytes()).get("Name");
            warn(it, et, cat(&[&pl::s("Error decoding "), &name]))?;
            return Ok(Value::Undef);
        }
    }
    if (!v.is_ref() && (re(r"\0", "").is_match(&v) || pl::gt(&pl::length(&v), &n(200)))) || pl::str_eq(tag, "XAttrMDLabel") {
        let buff = v;
        v = pl::sref(buff);
    }
    Ok(v)
}

/// Read MacOS XAttr value
/// Inputs: 0) ExifTool object ref, 1) file name
pub fn read_x_attr_value(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    ret(read_x_attr_value_impl(it, &arg(&a, 0), &arg(&a, 1), &arg(&a, 2), arg(&a, 3))?)
}

/// Read MacOS extended attribute tags using 'xattr' utility
/// Inputs: 0) ExifTool object ref, 1) file name
pub fn extract_x_attr_tags(it: &mut Interp, a: Vec<Value>, want: Want) -> ER<Vec<Value>> {
    pl::local_topic(it, |it| {
        let (et, file) = (arg(&a, 0), arg(&a, 1));
        let (mut tag, mut v, mut warn_) = (Value::Undef, Value::Undef, Value::Undef);

        let fname = escape_fn(&file); // escape necessary characters
        pl::method(it, &et, "VPrint", vec![n(0), pl::s("(running xattr)")])?;
        let cmd = cat(&[&pl::s("/usr/bin/xattr -lx \""), &fname, &pl::s("\" 2> /dev/null")]);
        let (mut xattr, status) = backticks_list(it, cmd)?; // get MacOS extended attributes
        if status.truthy() || xattr.is_empty() {
            if status.truthy() {
                warn(it, &et, pl::s("Error running \"xattr\" to extract XAttr tags"))?;
            }
            return pl::ret_nothing(want);
        }
        let tag_table_ptr = call(it, "GetTagTable", vec![pl::s("Image::ExifTool::MacOS::XAttr")])?;
        et.set("INDENT", cat(&[&et.get("INDENT"), &pl::s("| ")]));
        pl::method(it, &et, "VerboseDir", vec![pl::s("XAttr")])?;
        xattr.push(pl::s("")); // (for a list terminator)
        for mut l in xattr {
            chomp(&mut l);
            if re(r"^[\dA-Fa-f]{8}", "").subst(&mut l, false, |_| Vec::new()) > 0 {
                if !tag.truthy() {
                    warn_ = n(1);
                    continue;
                }
                re(r"\|.*", "").subst(&mut l, false, |_| Vec::new());
                pl::tr(&mut l, " ", "", "d");
                if re(r"[^\dA-Fa-f]", "").is_match(&l) || (pl::length(&l).to_i64() & 1) != 0 {
                    warn_ = n(2);
                    continue;
                }
                if v.is_undef() {
                    v = pl::s("");
                }
                v = pl::concat(&v, &pl::pack("H*", &[l])?);
                continue;
            } else if tag.truthy() && !v.is_undef() {
                v = read_x_attr_value_impl(it, &et, &tag_table_ptr, &tag, v)?;
                if !v.is_undef() {
                    handle_tag(it, &et, &tag_table_ptr, tag.clone(), v.clone(), &[])?;
                }
                tag = Value::Undef;
                v = Value::Undef;
            }
            if !pl::length(&l).truthy() {
                continue;
            }
            if re(r":$", "").subst(&mut l, false, |_| Vec::new()) == 0 {
                warn_ = n(3); // attribute name must have trailing ":"
                continue;
            }
            if !v.is_undef() {
                warn_ = n(4);
                v = Value::Undef;
            }
            // remove random ID after kMDLabel in tag ID
            tag = l;
            re(r"^com.apple.metadata:kMDLabel_.*", "s").subst(&mut tag, false, |_| b"com.apple.metadata:kMDLabel".to_vec());
        }
        if warn_.truthy() {
            warn(it, &et, cat(&[&pl::s("Error "), &warn_, &pl::s(" parsing \"xattr\" output")]))?;
        }
        let mut indent = et.get("INDENT");
        let r = re(r"\| $", "").subst(&mut indent, false, |_| Vec::new());
        et.set("INDENT", indent);
        ret(if r > 0 { n(1) } else { pl::s("") })
    })
}

/// Extract MacOS file creation date/time
/// Inputs: 0) ExifTool object ref, 1) file name
pub fn get_file_create_date(it: &mut Interp, a: Vec<Value>, want: Want) -> ER<Vec<Value>> {
    pl::local_topic(it, |it| {
        let (et, file) = (arg(&a, 0), arg(&a, 1));

        let fname = escape_fn(&file); // escape necessary characters
        pl::method(it, &et, "VPrint", vec![n(0), pl::s("(running stat)")])?;
        let cmd = cat(&[&pl::s("/usr/bin/stat -f '%SB' -t '%Y:%m:%d %H:%M:%S%z' \""), &fname, &pl::s("\" 2> /dev/null")]);
        let (mut time, status) = backticks(it, cmd)?;
        if status.truthy() || !time.truthy() || re(r"([-+]\d{2})(\d{2})\s*$", "").subst(&mut time, false, |c| [&cap(c, 1).to_bytes()[..], b":", &cap(c, 2).to_bytes()[..]].concat()) == 0 {
            warn(it, &et, pl::s("Error running \"stat\" to extract FileCreateDate"))?;
            return pl::ret_nothing(want);
        }
        et.set("SET_GROUP1", pl::s("MacOS"));
        pl::method(it, &et, "FoundTag", vec![pl::s("FileCreateDate"), time])?;
        ret(et.delete("SET_GROUP1"))
    })
}

/// Read ATTR metadata from "._" file
/// Inputs: 0) ExifTool ref, 1) dirInfo ref, 2) tag table ref
/// Return: 1 on success
/// (ref https://www.swiftforensics.com/2018/11/the-dot-underscore-file-format.html)
pub fn process_attr(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info, tag_table_ptr) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    let data_pt = dir_info.get("DataPt");
    let data_pos = dir_info.get("DataPos");
    pl::at(660);
    let data_len = pl::length(&pl::sderef(&data_pt)?);

    if !(pl::ge(&data_len, &n(58)) && re(r"^.{34}ATTR", "s").is_match(&data_pt.deref())) {
        warn(it, &et, pl::s("Invalid ATTR header"))?;
        return ret(n(0));
    }
    let entries = call(it, "Get32u", vec![data_pt.clone(), n(66)])?;
    pl::method(it, &et, "VerboseDir", vec![pl::s("ATTR"), entries.clone()])?;
    // (Note: The RAF is not in $dirInfo because it would break RSRC reading --
    // the RSCR block uses relative offsets, while the ATTR block uses absolute! grrr!)
    let _raf = et.get("RAF");
    let mut pos = n(70); // first entry is after ATTR header
    let mut i = n(0);
    while pl::lt(&i, &entries) {
        if pl::gt(&pl::add(&pos, &n(12)), &data_len) {
            warn(it, &et, pl::s("Truncated ATTR entry"))?;
            break;
        }
        let mut off = call(it, "Get32u", vec![data_pt.clone(), pos.clone()])?;
        let len = call(it, "Get32u", vec![data_pt.clone(), pl::add(&pos, &n(4))])?;
        let nn = call(it, "Get8u", vec![data_pt.clone(), pl::add(&pos, &n(10))])?; // number of characters in tag name
        if pl::gt(&pl::add(&pl::add(&pos, &n(11)), &nn), &data_len) {
            warn(it, &et, pl::s("Truncated ATTR name"))?;
            break;
        }
        off = pl::sub(&off, &data_pos); // convert to relative offset (grrr!)
        if !pl::lt(&off, &n(0)) && pl::gt(&off, &data_len) {
            warn(it, &et, pl::s("Invalid ATTR offset"))?;
            break;
        }
        let mut tag = substr(it, &data_pt.deref(), &pl::add(&pos, &n(11)), Some(&nn))?;
        re(r"\0+$", "").subst(&mut tag, false, |_| Vec::new()); // remove null terminator
        // remove random ID after kMDLabel in tag ID
        re(r"^com.apple.metadata:kMDLabel_.*", "s").subst(&mut tag, false, |_| b"com.apple.metadata:kMDLabel".to_vec());
        if pl::gt(&pl::add(&off, &len), &data_len) {
            warn(it, &et, pl::s("Truncated ATTR value"))?;
            break;
        }
        let s = substr(it, &data_pt.deref(), &off, Some(&len))?;
        let v = read_x_attr_value_impl(it, &et, &tag_table_ptr, &tag, s)?;
        if !v.is_undef() {
            handle_tag(it, &et, &tag_table_ptr, tag, v, &[("DataPt", data_pt.clone()), ("DataPos", data_pos.clone()), ("Start", off.clone()), ("Size", len.clone())])?;
        }
        // step to next entry (on even 4-byte boundary)
        pos = pl::add(&pos, &n((pl::add(&pl::add(&n(11), &nn), &n(3)).to_i64() as u64 & (-4i64 as u64)) as i64));
        i = pl::add(&i, &n(1));
    }
    ret(n(1))
}

/// Read information from a MacOS "._" sidecar file
/// Inputs: 0) ExifTool ref, 1) dirInfo ref
/// Returns: 1 on success, 0 if this wasn't a valid "._" file
/// (ref https://www.swiftforensics.com/2018/11/the-dot-underscore-file-format.html)
pub fn process_mac_os(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info) = (arg(&a, 0), arg(&a, 1));
    let raf = dir_info.get("RAF");
    let (hdr, buff) = (cell(Value::Undef), cell(Value::Undef));

    pl::at(706);
    check_invocant(&raf, "Read")?;
    if !pl::num_eq(&read(it, &raf, &hdr, n(26))?, &n(26)) {
        return ret(n(0));
    }
    let Some(m) = re(r"^\0\x05\x16\x07\0(.)\0\0Mac OS X        ", "s").m(&val(&hdr)) else {
        return ret(n(0));
    };
    let ver = n(cap(&m, 1).to_bytes()[0] as i64);
    // (extension may be anything, so just echo back the incoming file extension if it exists)
    pl::method(it, &et, "SetFileType", vec![Value::Undef, Value::Undef, et.get("FILE_EXT")])?;
    if !pl::num_eq(&ver, &n(2)) {
        warn(it, &et, cat(&[&pl::s("Unsupported file version "), &ver]))?;
        return ret(n(1));
    }
    call(it, "SetByteOrder", vec![pl::s("MM")])?;
    let tag_table_ptr = call(it, "GetTagTable", vec![pl::s("Image::ExifTool::MacOS::Main")])?;
    let entries = call(it, "Get16u", vec![rf(&hdr), n(0x18)])?;
    pl::method(it, &et, "VerboseDir", vec![pl::s("MacOS"), entries.clone()])?;
    let want = pl::mul(&entries, &n(12));
    if !pl::num_eq(&read(it, &raf, &hdr, want.clone())?, &want) {
        warn(it, &et, pl::s("Truncated header"))?;
        return ret(n(1));
    }
    let mut i = n(0);
    while pl::lt(&i, &entries) {
        let pos = pl::mul(&i, &n(12));
        let tag = call(it, "Get32u", vec![rf(&hdr), pos.clone()])?;
        let off = call(it, "Get32u", vec![rf(&hdr), pl::add(&pos, &n(4))])?;
        let len = call(it, "Get32u", vec![rf(&hdr), pl::add(&pos, &n(8))])?;
        if pl::gt(&len, &n(100000000)) {
            warn(it, &et, pl::s("Record size too large"))?;
            break;
        }
        if !(pl::method(it, &raf, "Seek", vec![off.clone(), n(0)])?.truthy() && pl::num_eq(&read(it, &raf, &buff, len.clone())?, &len)) {
            warn(it, &et, pl::s("Truncated record"))?;
            break;
        }
        handle_tag(it, &et, &tag_table_ptr, tag, Value::Undef, &[("DataPt", rf(&buff)), ("DataPos", off), ("Index", i.clone())])?;
        i = pl::add(&i, &n(1));
    }
    ret(n(1))
}

/// Register the subs ported so far; the others still run from the Perl.
pub fn register(it: &mut Interp) {
    it.define_native("Image::ExifTool::MacOS::MDItemLocalTime", md_item_local_time);
    it.define_native("Image::ExifTool::MacOS::System", system);
    it.define_native("Image::ExifTool::MacOS::SetMacOSTags", set_mac_os_tags);
    it.define_native("Image::ExifTool::MacOS::ExtractMDItemTags", extract_md_item_tags);
    it.define_native("Image::ExifTool::MacOS::ReadXAttrValue", read_x_attr_value);
    it.define_native("Image::ExifTool::MacOS::ExtractXAttrTags", extract_x_attr_tags);
    it.define_native("Image::ExifTool::MacOS::GetFileCreateDate", get_file_create_date);
    it.define_native("Image::ExifTool::MacOS::ProcessATTR", process_attr);
    it.define_native("Image::ExifTool::MacOS::ProcessMacOS", process_mac_os);
}
