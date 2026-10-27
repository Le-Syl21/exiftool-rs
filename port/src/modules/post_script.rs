//! Image::ExifTool::PostScript (Image/ExifTool/PostScript.pm), ported sub by sub (see docs/port/PORTING.md).
//!
//! Read PostScript meta information

// (the Perl assigns values it does not read again; the port keeps them)
#![allow(unused_assignments)]

use crate::interp::eval::{Cell, ER};
use crate::interp::{Interp, Want};
use crate::modules::random_access::{cell, raf_read_cell, substr};
use crate::perl::regex as pre;
use crate::perl::Value;
use crate::pl::{self, arg, cap, n, re, ret};
use crate::runtime;

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

fn set(c: &Cell, v: Value) {
    *c.borrow_mut() = v;
}

/// `$raf->Read($buff, $len)` into a `my` variable.
fn read(it: &mut Interp, raf: &Value, buff: &mut Value, len: Value) -> ER<Value> {
    let c = cell(buff.clone());
    let r = raf_read_cell(it, raf, &c, len)?;
    *buff = val(&c);
    Ok(r)
}

/// `$raf->ReadLine($buff)` into a `my` variable.
fn read_line(it: &mut Interp, raf: &Value, buff: &mut Value) -> ER<Value> {
    let mut a = [buff.clone()];
    let r = pl::method_mut(it, raf, "ReadLine", &mut a, Want::Scalar)?;
    *buff = a[0].clone();
    Ok(r.into_iter().next().unwrap_or_default())
}

fn seek(it: &mut Interp, raf: &Value, pos: Value, whence: i64) -> ER<bool> {
    Ok(pl::method(it, raf, "Seek", vec![pos, n(whence)])?.truthy())
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

/// `$/`
fn irs(it: &mut Interp) -> Value {
    it.get_scalar("main::/")
}

/// `shift @$lines`
fn shift(lines: &Value) -> Value {
    match lines {
        Value::ArrayRef(a) => {
            let mut a = a.borrow_mut();
            if a.is_empty() {
                Value::Undef
            } else {
                a.remove(0)
            }
        }
        _ => Value::Undef,
    }
}

/// `@$lines = LIST`
fn set_items(lines: &Value, items: Vec<Value>) {
    if let Value::ArrayRef(a) = lines {
        *a.borrow_mut() = items;
    }
}

/// `$s =~ /PAT/g` in scalar context from `pos` (None is undef): the match
/// spans, moving pos to its end, or None (pos reset).
fn match_g(pat: &[u8], flags: &str, s: &Value, pos: &mut Option<usize>) -> Option<Vec<Option<(usize, usize)>>> {
    let rx = pre::compile(pat, flags).expect("pattern");
    match rx.match_at(&s.to_bytes(), pos.unwrap_or(0)) {
        Ok(Some(sp)) => {
            *pos = sp[0].map(|(_, e)| e);
            Some(sp)
        }
        _ => {
            *pos = None;
            None
        }
    }
}

/// `oct($c)` of octal digits.
fn oct(c: &[u8]) -> i64 {
    c.iter().fold(0i64, |v, &d| v * 8 + (d - b'0') as i64)
}

/// `substr($s, $n, $len) = $r`
fn replace(s: &Value, at: usize, len: usize, r: &[u8]) -> Value {
    let b = s.to_bytes();
    let mut out = b[..at].to_vec();
    out.extend_from_slice(r);
    out.extend_from_slice(&b[(at + len).min(b.len())..]);
    Value::bytes(out)
}

/// AutoLoad our writer routines when necessary
pub fn autoload(it: &mut Interp, a: Vec<Value>, want: Want) -> ER<Vec<Value>> {
    let name = it.get_scalar("Image::ExifTool::PostScript::AUTOLOAD");
    let mut args = vec![name];
    args.extend(a);
    it.call_named("Image::ExifTool::DoAutoLoad", args, want)
}

/// Get image width or height
/// Inputs: 0) value list ref (ImageData, BoundingBox), 1) true to get height
pub fn image_size(_it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (vals, get_height) = (arg(&a, 0), arg(&a, 1));
    let (mut w, mut h) = (Value::Undef, Value::Undef);
    let (v0, v1) = (vals.at(0), vals.at(1));
    let m0 = if v0.truthy() { re(r"^(\d+) (\d+)", "").m(&v0) } else { None };
    if let Some(m) = m0 {
        (w, h) = (cap(&m, 1), cap(&m, 2));
    } else if let Some(m) = if v1.truthy() { re(r"^(\d+) (\d+) (\d+) (\d+)", "").m(&v1) } else { None } {
        (w, h) = (pl::sub(&cap(&m, 3), &cap(&m, 1)), pl::sub(&cap(&m, 4), &cap(&m, 2)));
    }
    ret(if get_height.truthy() { h } else { w })
}

fn ps_err_impl(it: &mut Interp, et: &Value, str_: &Value) -> ER<Value> {
    // set file type if not done already
    let ext = et.get("FILE_EXT");
    let t = if ext.truthy() && pl::str_eq(&ext, "AI") { "AI" } else { "PS" };
    pl::method(it, et, "SetFileType", vec![pl::s(t)])?;
    warn(it, et, cat(&[&pl::s("PostScript format error ("), str_, &pl::s(")")]))?;
    Ok(n(1))
}

/// Set PostScript format error warning
/// Inputs: 0) ExifTool object reference, 1) error string
/// Returns: 1
pub fn ps_err(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    ret(ps_err_impl(it, &arg(&a, 0), &arg(&a, 1))?)
}

fn get_input_record_separator_impl(it: &mut Interp, raf: &Value) -> ER<Value> {
    pl::at(195);
    let pos = pl::method(it, raf, "Tell", vec![])?; // save current position
    let mut data = Value::Undef;
    let mut sep = Value::Undef;
    if !read(it, raf, &mut data, n(256))?.truthy() {
        return Ok(Value::Undef);
    }
    let (mut a, mut d) = (999i64, 999i64);
    let b = data.to_bytes();
    if let Some(p) = b.iter().position(|&c| c == 0x0a) {
        a = p as i64 + 1;
    }
    if let Some(p) = b.iter().position(|&c| c == 0x0d) {
        d = p as i64 + 1;
    }
    let diff = a - d;
    if diff == 1 {
        sep = Value::bytes(b"\x0d\x0a");
    } else if diff == -1 {
        sep = Value::bytes(b"\x0a\x0d");
    } else if diff > 0 {
        sep = Value::bytes(b"\x0d");
    } else if diff < 0 {
        sep = Value::bytes(b"\x0a");
    } // else error
    seek(it, raf, pos, 0)?; // restore original position
    Ok(sep)
}

/// Return input record separator to use for the specified file
/// Inputs: 0) RAF reference
/// Returns: Input record separator or undef on error
pub fn get_input_record_separator(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    ret(get_input_record_separator_impl(it, &arg(&a, 0))?)
}

fn split_line_impl(data: &mut Value, lines: &Value) {
    loop {
        let b = data.to_bytes();
        // find the position of the first LF (\x0a)
        let mut endl: Option<usize> = b.iter().position(|&c| c == 0x0a).map(|p| p + 1);
        if let Some(p) = b.iter().position(|&c| c == 0x0d).map(|p| p + 1) {
            // find the first CR (\x0d)
            if let Some(e) = endl {
                // (remember, CR+LF is a DOS newline...)
                if (p as i64) < e as i64 - 1 {
                    endl = Some(p);
                }
            } else {
                endl = Some(p);
            }
        } else if endl.is_none() {
            lines.push(data.clone());
            break;
        }
        let endl = endl.unwrap();
        if b.len() == endl {
            lines.push(data.clone());
            break;
        } else {
            // continue to split into separate lines
            lines.push(Value::bytes(&b[..endl]));
            *data = Value::bytes(&b[endl..]);
        }
    }
}

/// Split into lines ending in any CR, LF or CR+LF combination
/// (this is annoying, and could be avoided if EPS files didn't mix linefeeds!)
/// Inputs: 0) data pointer, 1) reference to lines array
/// Notes: Fills @$lines with lines from splitting $$dataPt
pub fn split_line(_it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (data_pt, lines) = (arg(&a, 0), arg(&a, 1));
    pl::at(226);
    let mut data = pl::sderef(&data_pt)?;
    split_line_impl(&mut data, &lines);
    pl::set_deref(&data_pt, data)?;
    // (the value of the loop that ends the sub)
    Ok(vec![])
}

fn check_ps_end_impl(it: &mut Interp, raf: &Value, data: &mut Value) -> ER<Value> {
    let pos = pl::method(it, raf, "Tell", vec![])?;
    let ps_end = raf.get("PSEnd");
    if pl::ge(&pos, &ps_end) {
        let r = pl::method(it, raf, "Seek", vec![n(0), n(2)])?; // seek to end of file so we can't read any more
        if pl::gt(&pos, &ps_end) {
            let len = pl::add(&pl::sub(&pl::length(data), &pos), &ps_end);
            *data = substr(it, data, &n(0), Some(&len))?;
            return Ok(data.clone());
        }
        return Ok(r);
    }
    Ok(Value::from(false))
}

/// check to be sure we haven't read past end of PS data in DOS-style file
/// Inputs: 0) RAF ref (with PSEnd member), 1) data ref
/// - modifies data and sets RAF to EOF if end of PS is reached
pub fn check_ps_end(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (raf, data_pt) = (arg(&a, 0), arg(&a, 1));
    // ($$dataPt is read only when the data goes past the end)
    pl::at(256);
    let pos = pl::method(it, &raf, "Tell", vec![])?;
    let ps_end = raf.get("PSEnd");
    if pl::ge(&pos, &ps_end) {
        let r = pl::method(it, &raf, "Seek", vec![n(0), n(2)])?; // seek to end of file so we can't read any more
        if pl::gt(&pos, &ps_end) {
            pl::at(259);
            let data = pl::sderef(&data_pt)?;
            let len = pl::add(&pl::sub(&pl::length(&data), &pos), &ps_end);
            let v = substr(it, &data, &n(0), Some(&len))?;
            pl::set_deref(&data_pt, v.clone())?;
            return ret(v);
        }
        // (the value of the false statement modifier)
        let _ = r;
        return ret(Value::from(false));
    }
    ret(Value::from(false))
}

fn get_next_line_impl(it: &mut Interp, raf: &Value, lines: &Value) -> ER<Value> {
    let mut data = Value::Undef;
    let mut changed_nl = Value::Undef;
    let mut altnl: Value = Value::bytes(if pl::str_eq(&irs(it), "\x0d") { b"\x0a" } else { b"\x0d" });
    pl::at(274);
    loop {
        if !read_line(it, raf, &mut data)?.truthy() {
            break;
        }
        if raf.get("PSEnd").truthy() {
            check_ps_end_impl(it, raf, &mut data)?;
        }
        // split line if it contains other newline sequences
        let alt = altnl.to_bytes();
        if data.to_bytes().windows(alt.len().max(1)).any(|w| w == &alt[..]) {
            if pl::gt(&pl::length(&data), &n(500000)) && call(it, "IsPC", vec![])?.truthy() {
                // patch for Windows memory problem
                if !changed_nl.truthy() {
                    changed_nl = irs(it);
                    it.set_scalar("main::/", altnl.clone());
                    altnl = changed_nl.clone();
                    seek(it, raf, pl::neg(&pl::length(&data)), 1)?;
                    continue;
                }
            } else {
                // split into separate lines
                //    push @$lines, split /$altnl/, $data, -1;
                //    if (@$lines == 2 and $$lines[1] eq $/) {
                //        # handle case of DOS newline data inside file using Unix newlines
                //        $$lines[0] .= pop @$lines;
                //    }
                // split into separate lines if necessary
                split_line_impl(&mut data, lines);
            }
        } else {
            lines.push(data.clone());
        }
        if changed_nl.truthy() {
            it.set_scalar("main::/", changed_nl);
        }
        return Ok(n(1));
    }
    Ok(n(0))
}

/// Read next line from EPS file
/// Inputs: 0) RAF ref (with PSEnd member if Postscript ends before end of file)
///         1) array of lines from file
/// Returns: true on success
pub fn get_next_line(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    ret(get_next_line_impl(it, &arg(&a, 0), &arg(&a, 1))?)
}

/// The escape decoding loop of DecodeComment and UnescapePostScript
/// (`cont` adds UnescapePostScript's line continuations).
fn unescape_loop(s: &mut Value, cont: bool) {
    let mut pos: Option<usize> = None;
    while let Some(sp) = match_g(br"\\(.)", "s", s, &mut pos) {
        let p = pos.unwrap();
        let nn = p - 2;
        let (cs, ce) = sp[1].unwrap();
        let mut c = s.to_bytes()[cs..ce].to_vec();
        let r: Vec<u8>;
        if c[0].is_ascii_digit() && c[0] < b'8' {
            // get up to 2 more octal digits
            if let Some(sp2) = match_g(br"\G([0-7]{1,2})", "", s, &mut pos) {
                let (a, b) = sp2[1].unwrap();
                c.extend_from_slice(&s.to_bytes()[a..b]);
            }
            // convert octal escape code
            r = vec![(oct(&c) & 0xff) as u8];
        } else if cont && c[0] == 0x0d {
            // the string is continued if the line ends with '\'
            // (also remove "\x0d\x0a")
            if let Some(sp2) = match_g(br"\G(\x0a)", "", s, &mut pos) {
                let (a, b) = sp2[1].unwrap();
                c.extend_from_slice(&s.to_bytes()[a..b]);
            }
            r = Vec::new();
        } else if cont && c[0] == 0x0a {
            r = Vec::new();
        } else {
            // convert escaped characters
            let mut rv = Value::bytes(&c);
            pl::tr(&mut rv, "nrtbf", "\n\r\t\x08\x0c", "");
            r = rv.to_bytes().to_vec();
        }
        *s = replace(s, nn, c.len() + 1, &r);
        // continue search after this character
        pos = Some(nn + r.len());
    }
}

fn decode_comment_impl(it: &mut Interp, mut v: Value, raf: &Value, lines: &Value, data_pt: &Value) -> ER<Value> {
    re(r"\x0d*\x0a*$", "").subst(&mut v, false, |_| Vec::new()); // remove trailing CR, LF or CR/LF
    // check for continuation comments
    // (`@$lines or ...` dies when $lines is not an array reference)
    pl::at(318);
    if !lines.is_array() {
        pl::aderef(lines)?;
    }
    loop {
        if lines.is_empty() && !get_next_line_impl(it, raf, lines)?.truthy() {
            break;
        }
        if !re(r"^%%\+", "").is_match(&lines.at(0)) {
            break; // is the next line a continuation?
        }
        if data_pt.truthy() {
            pl::set_deref(data_pt, pl::concat(&data_pt.deref(), &lines.at(0)))?; // add to data if necessary
        }
        let mut l0 = lines.at(0);
        re(r"\x0d*\x0a*$", "").subst(&mut l0, false, |_| Vec::new()); // remove trailing CR, LF or CR/LF
        lines.set_at(0, l0);
        let first = shift(lines);
        v = pl::concat(&v, &pl::substr(&first, 3, None)); // add to value (without leading "%%+")
    }
    let mut vals: Vec<Value> = Vec::new();
    // handle bracketed string values
    if re(r"^\((.*)\)$", "").subst(&mut v, false, |c| cap(c, 1).to_bytes().to_vec()) > 0 {
        // remove brackets if necessary
        // split into an array of strings if necessary
        let mut nesting = 1i64;
        let mut pos: Option<usize> = None;
        while let Some(sp) = match_g(br"(\(|\))", "", &v, &mut pos) {
            let (bs, be) = sp[1].unwrap();
            let bra = v.to_bytes()[bs..be].to_vec();
            let p = pos.unwrap() as i64;
            let mut pp = p - 2;
            let mut backslashes = 0;
            while pp != 0 && pl::str_eq(&pl::substr(&v, pp, Some(1)), "\\") {
                pp -= 1;
                backslashes += 1;
            }
            if backslashes & 0x01 != 0 {
                continue; // escaped if odd number
            }
            if bra == b"(" {
                nesting += 1;
            } else {
                nesting -= 1;
                if nesting == 0 {
                    vals.push(pl::substr(&v, 0, Some(p - 1)));
                    v = pl::substr(&v, p, None);
                    pos = None;
                    if re(r"\s*\(", "").subst(&mut v, false, |_| Vec::new()) > 0 {
                        nesting += 1;
                    }
                }
            }
        }
        vals.push(v.clone());
        for x in vals.iter_mut() {
            // decode escape sequences in bracketed strings
            // (similar to code in PDF.pm, but without line continuation)
            unescape_loop(x, false);
        }
        v = if vals.len() > 1 { Value::new_array(vals) } else { vals[0].clone() };
    }
    Ok(v)
}

/// Decode comment from PostScript file
/// Inputs: 0) comment string, 1) RAF ref, 2) reference to lines array
///         3) optional data reference for extra lines read from file
/// Returns: Decoded comment string (may be an array reference)
/// - handles multi-line comments and escape sequences
pub fn decode_comment(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    ret(decode_comment_impl(it, arg(&a, 0), &arg(&a, 1), &arg(&a, 2), &arg(&a, 3))?)
}

fn unescape_post_script_impl(s: Value) -> Value {
    let mut s = s;
    // decode escape sequences in literal strings
    unescape_loop(&mut s, true);
    s
}

/// Unescape PostScript string
/// Inputs: 0) string
/// Returns: unescaped string
pub fn unescape_post_script(_it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    ret(unescape_post_script_impl(arg(&a, 0)))
}

fn process_ps_impl(it: &mut Interp, et: &Value, dir_info: &Value, tag_table_ptr: &Value) -> ER<Value> {
    let mut raf = dir_info.get("RAF");
    let embedded = pl::method(it, et, "Options", vec![pl::s("ExtractEmbedded")])?;
    let mut data = Value::Undef;
    let mut dos = Value::Undef;
    let mut font_table = Value::Undef;
    let mut comment = Value::Undef;

    // allow read from data
    if !raf.truthy() {
        raf = pl::method(it, &pl::s("File::RandomAccess"), "new", vec![dir_info.get("DataPt")])?;
        pl::method(it, et, "VerboseDir", vec![pl::s("PostScript")])?;
    }
    //
    // determine if this is a postscript file
    //
    if !pl::num_eq(&read(it, &raf, &mut data, n(4))?, &n(4)) {
        return Ok(n(0));
    }
    // accept either ASCII or DOS binary postscript file format
    if !re(r"^(%!PS|%!Ad|%!Fo|\xc5\xd0\xd3\xc6)", "").is_match(&data) {
        return Ok(n(0));
    }
    if re(r"^%!Ad", "").is_match(&data) {
        // I've seen PS files start with "%!Adobe-PS"...
        if !(pl::num_eq(&read(it, &raf, &mut data, n(6))?, &n(6)) && pl::str_eq(&data, "obe-PS")) {
            return Ok(n(0));
        }
    } else if re(r"^\xc5\xd0\xd3\xc6", "").is_match(&data) {
        // process DOS binary file header
        // - save DOS header then seek ahead and check PS header
        if !pl::num_eq(&read(it, &raf, &mut dos, n(26))?, &n(26)) {
            return Ok(n(0));
        }
        call(it, "SetByteOrder", vec![pl::s("II")])?;
        let ps_start = call(it, "Get32u", vec![pl::sref(dos.clone()), n(0)])?;
        if !(seek(it, &raf, ps_start.clone(), 0)? && pl::num_eq(&read(it, &raf, &mut data, n(4))?, &n(4)) && pl::str_eq(&data, "%!PS")) {
            return ps_err_impl(it, et, &pl::s("invalid header"));
        }
        let len = call(it, "Get32u", vec![pl::sref(dos.clone()), n(4)])?;
        raf.set("PSEnd", pl::add(&ps_start, &len)); // set end of PostScript data in RAF
    } else {
        // check for PostScript font file (PFA or PFB)
        let mut d2 = Value::Undef;
        if read(it, &raf, &mut d2, n(12))?.truthy() {
            data = pl::concat(&data, &d2);
        }
        if re(r"^%!(PS-(AdobeFont-|Bitstream )|FontType1-)", "").is_match(&data) {
            pl::method(it, et, "SetFileType", vec![pl::s("PFA")])?; // PostScript ASCII font file
            font_table = call(it, "GetTagTable", vec![pl::s("Image::ExifTool::Font::PSInfo")])?;
            // PostScript font files may contain an unformatted comments which may
            // contain useful information, so accumulate these for the Comment tag
            comment = n(1);
        }
        seek(it, &raf, pl::neg(&pl::length(&data)), 1)?;
    }
    //
    // set the newline type based on the first newline found in the file
    //
    let sep = get_input_record_separator_impl(it, &raf)?;
    pl::local_global(it, b'$', "main::/", |it| {
        it.set_scalar("main::/", sep);
        if !irs(it).truthy() {
            return ps_err_impl(it, et, &pl::s("invalid PS data"));
        }
        process_ps_body(it, et, &raf, tag_table_ptr, &embedded, data, dos, font_table, comment)
    })
}

/// The part of ProcessPS under `local $/`.
#[allow(clippy::too_many_arguments)]
fn process_ps_body(it: &mut Interp, et: &Value, raf: &Value, tag_table_ptr: &Value, embedded: &Value, mut data: Value, dos: Value, mut font_table: Value, mut comment: Value) -> ER<Value> {
    // set file type (PostScript or EPS)
    if !read_line(it, raf, &mut data)?.truthy() {
        data = pl::s("");
    }
    let type_;
    if re(r"EPSF", "").is_match(&data) {
        type_ = "EPS";
    } else {
        // read next line to see if this is an Illustrator file
        let mut line2 = Value::Undef;
        let pos = pl::method(it, raf, "Tell", vec![])?;
        if read_line(it, raf, &mut line2)?.truthy() && re(r"^%%Creator: Adobe Illustrator", "").is_match(&line2) {
            type_ = "AI";
        } else {
            type_ = "PS";
        }
        seek(it, raf, pos, 0)?;
    }
    pl::method(it, et, "SetFileType", vec![pl::s(type_)])?;
    let fs = et.get("OPTIONS").get("FastScan");
    if fs.truthy() && pl::num_eq(&fs, &n(3)) {
        return Ok(n(1));
    }
    //
    // extract TIFF information from DOS header
    //
    let tag_table_ptr = if tag_table_ptr.truthy() { tag_table_ptr.clone() } else { call(it, "GetTagTable", vec![pl::s("Image::ExifTool::PostScript::Main")])? };
    if dos.truthy() {
        let base = call(it, "Get32u", vec![pl::sref(dos.clone()), n(16)])?;
        if base.truthy() {
            let pos = pl::method(it, raf, "Tell", vec![])?;
            // extract the TIFF preview
            let len = call(it, "Get32u", vec![pl::sref(dos.clone()), n(20)])?;
            let v = pl::method(it, et, "ExtractBinary", vec![base.clone(), len, pl::s("TIFFPreview")])?;
            if !v.is_undef() && re(r"^(MM\0\x2a|II\x2a\0|Binary)", "").is_match(&v) {
                pl::method(it, et, "HandleTag", vec![tag_table_ptr.clone(), pl::s("TIFFPreview"), v])?;
            } else {
                warn(it, et, pl::s("Bad TIFF preview image"))?;
            }
            // extract information from TIFF in DOS header
            // (set Parent to '' to avoid setting FileType tag again)
            let d = Value::new_hash();
            d.set("Parent", pl::s(""));
            d.set("RAF", raf.clone());
            d.set("Base", base);
            if !pl::method(it, et, "ProcessTIFF", vec![d])?.truthy() {
                warn(it, et, pl::s("Bad embedded TIFF"))?;
            }
            // position file pointer to extract PS information
            seek(it, raf, pos, 0)?;
        }
    }
    //
    // parse the postscript
    //
    let buff: Cell = cell(Value::Undef);
    let (mut mode, mut begin_token, mut end_token, mut doc_num, mut sub_doc_num, mut changed_nl) = (Value::Undef, Value::Undef, Value::Undef, Value::Undef, Value::Undef, Value::Undef);
    let mut end_doc = Value::Undef;
    let lines = Value::new_array(Vec::new());
    let mut altnl: Value;
    if pl::str_eq(&irs(it), "\x0d") {
        altnl = pl::s("\x0a");
    } else {
        it.set_scalar("main::/", pl::s("\x0a")); // end on any LF (even if DOS CR+LF)
        altnl = pl::s("\x0d");
    }
    let xpacket_end = re(r"<\?xpacket end=.(w|r).\?>(\n|\r|$)", "");
    loop {
        if !lines.is_empty() {
            data = shift(&lines);
        } else {
            if !read_line(it, raf, &mut data)?.truthy() {
                break;
            }
            // check for alternate newlines as efficiently as possible
            let alt_re = pl::re_dyn(&altnl.to_bytes(), "")?;
            if alt_re.is_match(&data) {
                if pl::gt(&pl::length(&data), &n(500000)) && call(it, "IsPC", vec![])?.truthy() {
                    // Windows can't split very long lines due to poor memory handling,
                    // so re-read the file with the other newline character instead
                    // (slower but uses less memory)
                    if !changed_nl.truthy() {
                        changed_nl = n(1);
                        let t = irs(it);
                        it.set_scalar("main::/", altnl.clone());
                        altnl = t;
                        seek(it, raf, pl::neg(&pl::length(&data)), 1)?;
                        continue;
                    }
                } else {
                    // split into separate lines
                    let parts = pl::split(&alt_re, &data, -1);
                    set_items(&lines, parts);
                    data = shift(&lines);
                    if lines.len() == 1 && lines.at(0).to_bytes() == irs(it).to_bytes() {
                        // handle case of DOS newline data inside file using Unix newlines
                        data = pl::concat(&data, &lines.at(0));
                        set_items(&lines, Vec::new());
                    }
                }
            }
        }
        changed_nl = Value::Undef;
        if mode.truthy() {
            if !end_token.truthy() {
                set(&buff, pl::concat(&val(&buff), &data));
                if !xpacket_end.is_match(&data) {
                    continue;
                }
            } else if !pl::re_dyn(&[b"^", &end_token.to_bytes()[..]].concat(), "i")?.is_match(&data) {
                if pl::str_eq(&mode, "XMP") {
                    set(&buff, pl::concat(&val(&buff), &data));
                } else if pl::str_eq(&mode, "Document") {
                    // ignore embedded documents, but keep track of nesting level
                    if pl::re_dyn(&[b"^", &begin_token.to_bytes()[..]].concat(), "")?.is_match(&data) {
                        doc_num = pl::concat(&doc_num, &pl::s("-1"));
                    }
                } else {
                    // data is ASCII-hex encoded
                    pl::tr(&mut data, "0-9A-Fa-f", "", "dc"); // remove all but hex characters
                    let h = pl::pack("H*", &[data.clone()])?;
                    set(&buff, pl::concat(&val(&buff), &h)); // translate from hex
                }
                continue;
            } else if pl::str_eq(&mode, "Document") {
                re(r"-?\d+$", "").subst(&mut doc_num, false, |_| Vec::new()); // decrement document nesting level
                // done with Document mode if we are back at the top level
                if !doc_num.truthy() {
                    mode = Value::Undef;
                }
                continue;
            }
        } else if end_doc.truthy() && pl::re_dyn(&[b"^", &end_doc.to_bytes()[..]].concat(), "i")?.is_match(&data) {
            let mut one = Value::Undef;
            re(r"-?(\d+)$", "").subst(&mut doc_num, false, |c| {
                one = cap(c, 1);
                Vec::new()
            }); // decrement nesting level
            sub_doc_num = one; // remember our last sub-document number
            et.set("DOC_NUM", doc_num.clone());
            if !doc_num.truthy() {
                end_doc = Value::Undef; // done with document if top level
            }
            continue;
        } else if let Some(m) = re(r"^(%{1,2})(Begin)(_xml_packet|Photoshop|ICCProfile|Document|Binary)", "i").m(&data) {
            // the beginning of a data block
            let three = pl::lc(&cap(&m, 3));
            mode = match &*three.to_bytes() {
                b"_xml_packet" => pl::s("XMP"),
                b"photoshop" => pl::s("Photoshop"),
                b"iccprofile" => pl::s("ICC_Profile"),
                b"document" => pl::s("Document"),
                _ => Value::Undef, // (binary: we will try to skip this)
            };
            if !mode.truthy() {
                if lines.is_empty() {
                    if let Some(m2) = re(r"^%{1,2}BeginBinary:\s*(\d+)", "i").m(&data) {
                        if !seek(it, raf, cap(&m2, 1), 1)? {
                            break; // skip binary data
                        }
                    }
                }
                continue;
            }
            set(&buff, pl::s(""));
            begin_token = cat(&[&cap(&m, 1), &cap(&m, 2), &cap(&m, 3)]);
            end_token = cat(&[&cap(&m, 1), &pl::s(if pl::str_eq(&cap(&m, 2), "begin") { "end" } else { "End" }), &cap(&m, 3)]);
            if pl::str_eq(&mode, "Document") {
                // this is either the 1st sub-document or Nth document
                if doc_num.truthy() {
                    // increase nesting level
                    sub_doc_num = pl::add(&sub_doc_num, &n(1));
                    doc_num = cat(&[&doc_num, &pl::s("-"), &sub_doc_num]);
                } else {
                    // this is the Nth document
                    doc_num = pl::add(&et.get("DOC_COUNT"), &n(1));
                }
                sub_doc_num = n(0); // new level, so reset subDocNum
                if !embedded.truthy() {
                    continue; // skip over this document
                }
                // set document number for family 4-7 group names
                et.set("DOC_NUM", doc_num.clone());
                et.set("LIST_TAGS", Value::new_hash()); // don't build lists across different documents
                et.set("PROCESSED", Value::new_hash()); // re-initialize processed directory lookup too
                end_doc = end_token.clone(); // parse to EndDocument token
                // reset mode to allow parsing into sub-directories
                end_token = Value::Undef;
                mode = Value::Undef;
                // save document name if available
                let p = [b"^", &begin_token.to_bytes()[..], br":\s+([^\n\r]+)"].concat();
                if let Some(m) = pl::re_dyn(&p, "i")?.m(&data) {
                    let mut doc_name = cap(&m, 1);
                    // remove brackets if necessary
                    if let Some(m) = re(r"^\((.*)\)$", "").m(&doc_name) {
                        doc_name = cap(&m, 1);
                    }
                    pl::method(it, et, "HandleTag", vec![tag_table_ptr.clone(), pl::s("EmbeddedFileName"), doc_name])?;
                }
            }
            continue;
        } else if re(r"^<\?xpacket begin=.{7,13}W5M0MpCehiHzreSzNTczkc9d", "").is_match(&data) {
            // pick up any stray XMP data
            mode = pl::s("XMP");
            set(&buff, data.clone());
            end_token = Value::Undef; // no end token (just look for xpacket end)
            // XMP could be contained in a single line (if newlines are different)
            if !xpacket_end.is_match(&data) {
                continue;
            }
        } else if let Some(m) = re(r"^%%?(\w+): ?(.*)", "s").m(&data).filter(|m| tag_table_ptr.get(cap(m, 1).to_bytes()).truthy()) {
            let (tag, v) = (cap(&m, 1), cap(&m, 2));
            // only allow 'ImageData' and AI tags to have single leading '%'
            if !(re(r"^%(%|AI\d+_)", "").is_match(&data) || pl::str_eq(&tag, "ImageData")) {
                continue;
            }
            // decode comment string (reading continuation lines if necessary)
            let v = decode_comment_impl(it, v, raf, &lines, &Value::Undef)?;
            pl::method(it, et, "HandleTag", vec![tag_table_ptr.clone(), tag, v])?;
            continue;
        } else if embedded.truthy() && re(r"^%AI12_CompressedData", "").is_match(&data) {
            // the rest of the file is compressed
            if pl::eval_block(it, |it| runtime::require(it, "Compress::Zlib"))?.is_none() {
                warn(it, et, pl::s("Install Compress::Zlib to extract compressed embedded data"))?;
                break;
            }
            // seek back to find the start of the compressed data in the file
            let mut tlen = pl::add(&pl::length(&data), &n(lines.len() as i64));
            for l in lines.items() {
                tlen = pl::add(&tlen, &pl::length(&l));
            }
            let mut back_to = pl::sub(&pl::sub(&pl::method(it, raf, "Tell", vec![])?, &tlen), &n(64));
            if pl::lt(&back_to, &n(0)) {
                back_to = n(0);
            }
            if !(seek(it, raf, back_to, 0)? && read(it, raf, &mut data, n(2048))?.truthy()) {
                break;
            }
            if re(r".*?%AI12_CompressedData", "").subst(&mut data, false, |_| Vec::new()) == 0 {
                break;
            }
            let inflate = pl::call(it, "Compress::Zlib::inflateInit", vec![])?;
            if !inflate.truthy() {
                warn(it, et, pl::s("Error initializing inflate"))?;
                break;
            }
            // generate a PS-like file in memory from the compressed data
            let verbose = pl::method(it, et, "Options", vec![pl::s("Verbose")])?;
            if pl::gt(&verbose, &n(1)) {
                pl::method(it, et, "VerboseDir", vec![pl::s("AI12_CompressedData (first 4kB)")])?;
                pl::method(it, et, "VerboseDump", vec![pl::sref(data.clone())])?;
            }
            // remove header if it exists (Windows AI files only)
            re(r"^.{0,256}EndData[\x0d\x0a]+", "s").subst(&mut data, false, |_| Vec::new());
            let mut v = Value::Undef;
            let z_stream_end = pl::call(it, "Compress::Zlib::Z_STREAM_END", vec![])?;
            let z_ok = pl::call(it, "Compress::Zlib::Z_OK", vec![])?;
            loop {
                let mut args = [data.clone()];
                let r = pl::method_mut(it, &inflate, "inflate", &mut args, Want::List)?;
                data = args[0].clone();
                let (v2, stat) = (arg(&r, 0), arg(&r, 1));
                if pl::num_eq(&stat, &z_stream_end) {
                    v = pl::concat(&v, &v2);
                    break;
                }
                if !pl::num_eq(&stat, &z_ok) {
                    v = Value::Undef;
                    break;
                }
                if !v.is_undef() {
                    v = pl::concat(&v, &v2);
                } else if re(r"^%!PS", "").is_match(&v2) {
                    v = v2;
                } else {
                    // add postscript header (for file recognition) if it doesn't exist
                    v = cat(&[&pl::s("%!PS-Adobe-3.0"), &irs(it), &v2]);
                }
                if !read(it, raf, &mut data, n(65536))?.truthy() {
                    break;
                }
            }
            if v.is_undef() {
                warn(it, et, pl::s("Error inflating AI compressed data"))?;
                break;
            }
            if pl::gt(&verbose, &n(1)) {
                pl::method(it, et, "VerboseDir", vec![pl::s("Uncompressed AI12 Data")])?;
                pl::method(it, et, "VerboseDump", vec![pl::sref(v.clone())])?;
            }
            // extract information from embedded images in the uncompressed data
            // add PS header in case it needs one
            let d = Value::new_hash();
            d.set("DataPt", pl::sref(v));
            process_ps_impl(it, et, &d, &Value::Undef)?;
            break;
        } else if font_table.truthy() {
            if !comment.is_undef() {
                // extract initial comments from PostScript Font files
                if let Some(m) = re(r"^%\s+(.*?)[\x0d\x0a]", "").m(&data) {
                    if comment.truthy() {
                        comment = pl::concat(&comment, &pl::s("\n"));
                    }
                    comment = pl::concat(&comment, &cap(&m, 1));
                    continue;
                } else if !re(r"^%", "").is_match(&data) {
                    // stop extracting comments at the first non-comment line
                    if pl::length(&comment).truthy() {
                        pl::method(it, et, "FoundTag", vec![pl::s("Comment"), comment.clone()])?;
                    }
                    comment = Value::Undef;
                }
            }
            if let Some(m) = re(r"^\s*/(\w+)\s*(.*)", "").m(&data).filter(|m| font_table.get(cap(m, 1).to_bytes()).truthy()) {
                let (tag, mut v) = (cap(&m, 1), cap(&m, 2));
                if let Some(m) = re(r"^\((.*)\)", "").m(&v) {
                    v = unescape_post_script_impl(cap(&m, 1));
                } else if let Some(m) = re(r"/?(\S+)", "").m(&v) {
                    v = cap(&m, 1);
                }
                pl::method(it, et, "HandleTag", vec![font_table.clone(), tag, v])?;
            } else if re(r"^currentdict end", "").is_match(&data) {
                // only extract tags from initial FontInfo dict
                font_table = Value::Undef;
            }
            continue;
        } else {
            continue;
        }
        // extract information from buffered data
        let d = Value::new_hash();
        d.set("DataPt", rf(&buff));
        d.set("DataLen", pl::length(&val(&buff)));
        d.set("DirStart", n(0));
        d.set("DirLen", pl::length(&val(&buff)));
        d.set("Parent", pl::s("PostScript"));
        let sub_table_ptr = call(it, "GetTagTable", vec![cat(&[&pl::s("Image::ExifTool::"), &mode, &pl::s("::Main")])])?;
        if !pl::method(it, et, "ProcessDirectory", vec![d, sub_table_ptr])?.truthy() {
            warn(it, et, cat(&[&pl::s("Error processing "), &mode, &pl::s(" information in PostScript file")]))?;
        }
        set(&buff, Value::Undef);
        mode = Value::Undef;
    }
    if end_doc.truthy() && !mode.truthy() {
        mode = pl::s("Document");
    }
    if mode.truthy() {
        ps_err_impl(it, et, &cat(&[&pl::s("unterminated "), &mode, &pl::s(" data")]))?;
    }
    Ok(n(1))
}

/// Extract information from EPS, PS or AI file
/// Inputs: 0) ExifTool object reference, 1) dirInfo reference, 2) optional tag table ref
/// Returns: 1 if this was a valid PostScript file
pub fn process_ps(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    ret(process_ps_impl(it, &arg(&a, 0), &arg(&a, 1), &arg(&a, 2))?)
}

/// Extract information from EPS file
/// Inputs: 0) ExifTool object reference, 1) dirInfo reference
/// Returns: 1 if this was a valid PostScript file
pub fn process_eps(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    ret(process_ps_impl(it, &arg(&a, 0), &arg(&a, 1), &Value::Undef)?)
}

/// Register the subs ported so far; the others still run from the Perl.
pub fn register(it: &mut Interp) {
    it.define_native("Image::ExifTool::PostScript::AUTOLOAD", autoload);
    it.define_native("Image::ExifTool::PostScript::ImageSize", image_size);
    it.define_native("Image::ExifTool::PostScript::PSErr", ps_err);
    it.define_native("Image::ExifTool::PostScript::GetInputRecordSeparator", get_input_record_separator);
    it.define_native("Image::ExifTool::PostScript::SplitLine", split_line);
    it.define_native("Image::ExifTool::PostScript::CheckPSEnd", check_ps_end);
    it.define_native("Image::ExifTool::PostScript::GetNextLine", get_next_line);
    it.define_native("Image::ExifTool::PostScript::DecodeComment", decode_comment);
    it.define_native("Image::ExifTool::PostScript::UnescapePostScript", unescape_post_script);
    it.define_native("Image::ExifTool::PostScript::ProcessPS", process_ps);
    it.define_native("Image::ExifTool::PostScript::ProcessEPS", process_eps);
}
