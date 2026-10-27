//! Image::ExifTool::RTF (Image/ExifTool/RTF.pm), ported sub by sub (see docs/port/PORTING.md).
//!
//! Read Rich Text Format meta information. ReadToNested continues a match
//! where the caller's `pos()` left it and leaves `pos()` after the closing
//! brace, so the buffers handed to it keep their position in the
//! interpreter (read and set with `pos_of` / `set_pos`); the file's
//! `my %rtfEntity` stays Perl data, read with `lex_hash`.

use std::rc::Rc;

use crate::interp::eval::{Cell, ER};
use crate::interp::{Interp, Want};
use crate::modules::random_access::{cell, perl_eval, raf_read};
use crate::perl::regex::{self as pre, PerlRegex, Spans};
use crate::perl::Value;
use crate::pl::{self, arg, cap, n, re, ret};
use crate::runtime;

const MODULE: &str = "Image::ExifTool::RTF";

fn s(x: &str) -> Value {
    pl::s(x)
}

/// A compiled pattern, for the matches that use `pos()` and `\G`.
fn rx(pat: &[u8], flags: &str) -> ER<Rc<PerlRegex>> {
    pre::compile(pat, flags).map_err(|e| crate::interp::Flow::Die(Value::from(format!("{e}\n"))))
}

/// A match starting at `pos` (where `\G` anchors).
fn m_at(r: &PerlRegex, subject: &[u8], pos: usize) -> Option<Spans> {
    r.match_at(subject, pos).ok().flatten()
}

/// Capture `i` of a match as a value (undef when it did not take part).
fn sp(subject: &[u8], spans: &Spans, i: usize) -> Value {
    match spans.get(i).copied().flatten() {
        Some((a, b)) => Value::bytes(&subject[a..b]),
        None => Value::Undef,
    }
}

/// `pos($$dataPt)`
fn pos_of(it: &mut Interp, data_pt: &Value) -> ER<Option<usize>> {
    let c = cell(data_pt.clone());
    let v = perl_eval(it, "pos($$d)", MODULE, &[("d", &c)], Want::Scalar)?.pop().unwrap_or_default();
    Ok(if v.is_undef() { None } else { Some(v.to_i64().max(0) as usize) })
}

/// `pos($$dataPt) = $p` (undef to reset it)
fn set_pos(it: &mut Interp, data_pt: &Value, p: Option<usize>) -> ER<()> {
    let (c, pc) = (cell(data_pt.clone()), cell(p.map_or(Value::Undef, |p| n(p as i64))));
    perl_eval(it, "pos($$d) = $p", MODULE, &[("d", &c), ("p", &pc)], Want::Void)?;
    Ok(())
}

/// `$$dataPt =~ /PAT/g` in scalar context, on a scalar whose `pos()` the
/// interpreter keeps: the match starts at `pos()`, which is set to its end
/// (or reset on failure).
fn gmatch(it: &mut Interp, data_pt: &Value, r: &PerlRegex) -> ER<Option<(Vec<u8>, Spans)>> {
    let start = pos_of(it, data_pt)?.unwrap_or(0);
    let d = data_pt.deref().to_bytes().to_vec();
    match m_at(r, &d, start) {
        Some(m) => {
            set_pos(it, data_pt, Some(m[0].unwrap().1))?;
            Ok(Some((d, m)))
        }
        None => {
            set_pos(it, data_pt, None)?;
            Ok(None)
        }
    }
}

/// `$$dataPt = $val` (an assignment resets `pos()`).
fn assign(it: &mut Interp, data_pt: &Value, val: Value) -> ER<()> {
    pl::set_deref(data_pt, val)?;
    set_pos(it, data_pt, None)
}

//------------------------------------------------------------------------------
// Read to nested closing curly bracket "}"
// Inputs: 0) data ref, 1) optional RAF ref to read more data if available
// Returns: text inside brackets, or undef on error
// Notes: On entry the current position in the data must be set to immediately
//        after the command that opens the bracket.  On return the current
//        position is immediately following the closing brace if the return
//        value is defined.
pub fn read_to_nested(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (data_pt, raf) = (arg(&a, 0), arg(&a, 1));
    let pos = pos_of(it, &data_pt)?.unwrap_or(0);
    let mut level = 1;
    let bracket = rx(br"(\\*)([{}])", "")?;
    let mut cur: Option<usize> = Some(pos); // pos($$dataPt), kept here until we return
    loop {
        // look for the next bracket
        let d = data_pt.deref().to_bytes().to_vec();
        let Some(m) = m_at(&bracket, &d, cur.unwrap_or(0)) else {
            cur = None;
            // must read some more data
            let mut p = d.len();
            let mut buff = Value::Undef;
            if !(raf.truthy() && raf_read(it, &raf, &mut buff, n(65536))?.truthy()) {
                break;
            }
            let mut nd = d;
            nd.extend_from_slice(&buff.to_bytes());
            pl::set_deref(&data_pt, Value::bytes(&nd))?;
            // rewind position to include any leading backslashes
            while p > 0 && nd[p - 1] == b'\\' {
                p -= 1;
            }
            cur = Some(p); // set position to continue search
            continue;
        };
        cur = Some(m[0].unwrap().1);
        // bracket is escaped if preceded by an odd number of backslashes
        let g1 = sp(&d, &m, 1);
        if g1.truthy() && pl::length(&g1).to_i64() & 0x01 != 0 {
            continue;
        }
        if pl::str_eq(&sp(&d, &m, 2), "{") {
            level += 1;
            continue;
        }
        level -= 1;
        if level > 0 {
            continue;
        }
        let end = cur.unwrap();
        set_pos(it, &data_pt, cur)?;
        return ret(pl::substr(&data_pt.deref(), pos as i64, Some(end as i64 - pos as i64 - 1)));
    }
    set_pos(it, &data_pt, cur)?;
    pl::ret_undef()
}

//------------------------------------------------------------------------------
// Unescape RTF escape sequences
// Inputs: 0) ExifTool ref, 1) RTF text, 2) RTF character set (for hex characters)
// Returns: Unescaped text (in current ExifTool Charset)
pub fn unescape_rtf(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, mut val, charset) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));

    // return now unless we have a control sequence
    if !re(r"\\", "").is_match(&val) {
        pl::tr(&mut val, "\n\r", "", "d"); // ignore CR's and LF's
        return ret(val);
    }
    // CR/LF is significant if it terminates a control sequence (so change these to a space)
    // (was $val =~ s/(^|[^\\])((?:\\\\)*)(\\[a-zA-Z]+(?:-?\d+)?)[\n\r]/$1$2$3 /g;)
    re(r"\\(?:([a-zA-Z]+(?:-?\d+)?)[\n\r]|(.))", "s").subst(&mut val, true, |c| {
        let mut r = b"\\".to_vec();
        if cap(c, 1).truthy() {
            r.extend_from_slice(&cap(c, 1).to_bytes());
            r.push(b' ');
        } else {
            r.extend_from_slice(&cap(c, 2).to_bytes());
        }
        r
    });
    // protect the newline control sequence by converting to a \par command
    // (was $val =~ s/(^|[^\\])((?:\\\\)*)(\\[\n\r])/$1$2\\par /g;)
    re(r"(\\[\n\r])|(\\.)", "s").subst(&mut val, true, |c| {
        let c2 = cap(c, 2);
        if c2.truthy() {
            c2.to_bytes().to_vec()
        } else {
            br"\par ".to_vec()
        }
    });
    // all other CR/LF's are ignored (so delete them)
    pl::tr(&mut val, "\n\r", "", "d");

    let rtf_entity = runtime::lex_hash(it, MODULE, "rtfEntity");
    let mut rtn_val = Value::from("");
    let v = val.to_bytes().to_vec();
    let len = v.len();
    let mut skip = n(1); // default Unicode skip count
    let mut p0: Option<usize> = Some(0);
    let mut vpos: Option<usize> = None; // pos($val)
    let bs = rx(br"\\", "")?;
    let word = rx(br"\G([a-zA-Z]+)(-?\d+)? ?", "")?;

    loop {
        // find next backslash
        let p1 = match m_at(&bs, &v, vpos.unwrap_or(0)) {
            Some(m) => {
                vpos = Some(m[0].unwrap().1);
                vpos.unwrap()
            }
            None => {
                vpos = None;
                len + 1
            }
        };
        // add text up to start of this control sequence (or up to end)
        let p0v = p0.unwrap_or(0) as i64;
        let nn = p1 as i64 - p0v - 1;
        if nn > 0 {
            rtn_val = pl::concat(&rtn_val, &pl::substr(&val, p0v, Some(nn)));
        }
        // all done if at the end or if control sequence is empty
        if p1 >= len {
            break;
        }
        // look for an ASCII-letter control word or Unicode control
        if let Some(m) = m_at(&word, &v, vpos.unwrap_or(0)) {
            vpos = Some(m[0].unwrap().1);
            let (c1, c2) = (sp(&v, &m, 1), sp(&v, &m, 2));
            // interpret command if recognized
            if pl::str_eq(&c1, "uc") {
                // \ucN
                skip = c2;
            } else if pl::str_eq(&c1, "u") {
                // \uN
                if pl::lt(&c2, &n(0)) {
                    pl::method(it, &et, "Warn", vec![s("Invalid Unicode character(s) in text")])?;
                    rtn_val = pl::concat(&rtn_val, &s("?"));
                } else {
                    runtime::require(it, "Image::ExifTool::Charset")?;
                    let r = pl::call(it, "Image::ExifTool::Charset::Recompose", vec![et.clone(), Value::new_array(vec![c2])])?;
                    rtn_val = pl::concat(&rtn_val, &r);
                    if skip.truthy() {
                        // must skip the specified number of characters
                        // (not simple because RTF control words count as a single character)
                        let mut p = br"\G([^\\]|\\([a-zA-Z]+)(-?\d+)? ?|\\'.{2}|\\.){".to_vec();
                        p.extend_from_slice(&skip.to_bytes());
                        p.push(b'}');
                        let r = rx(&p, "")?;
                        match m_at(&r, &v, vpos.unwrap_or(0)) {
                            Some(m) => vpos = Some(m[0].unwrap().1),
                            None => {
                                break;
                            }
                        }
                    }
                }
            } else if rtf_entity.get(c1.to_bytes()).truthy() {
                runtime::require(it, "Image::ExifTool::Charset")?;
                let r = pl::call(it, "Image::ExifTool::Charset::Recompose", vec![et.clone(), Value::new_array(vec![rtf_entity.get(c1.to_bytes())])])?;
                rtn_val = pl::concat(&rtn_val, &r);
            } // (else ignore the command)
        } else {
            // (the failed match reset pos($val); both branches below set it again)
            let ch = pl::substr(&val, p1 as i64, Some(1));
            if pl::str_eq(&ch, "'") {
                // hex character code
                if p1 + 3 > len {
                    break;
                }
                let hex = pl::substr(&val, p1 as i64 + 1, Some(2));
                if re(r"^[0-9a-fA-F]{2}$", "").is_match(&hex) {
                    runtime::require(it, "Image::ExifTool::Charset")?;
                    let code = u8::from_str_radix(&hex.to_text(), 16).unwrap_or(0);
                    let d = pl::method(it, &et, "Decode", vec![Value::bytes([code]), charset.clone()])?;
                    rtn_val = pl::concat(&rtn_val, &d);
                }
                vpos = Some(p1 + 3); // skip to after the hex code
            } else {
                // assume a standard control symbol (\, {, }, etc)
                // (note, this may not be valid for some uncommon
                //  control symbols like \~ for non-breaking space)
                rtn_val = pl::concat(&rtn_val, &ch);
                vpos = Some(p1 + 1); // skip to after this character
            }
        }
        p0 = vpos;
    }
    ret(rtn_val)
}

/// `ReadToNested(\$var, $raf)` on a `my` variable kept in a cell.
fn read_to_nested_cell(it: &mut Interp, c: &Cell, raf: Option<&Value>) -> ER<Value> {
    let mut args = vec![Value::ScalarRef(c.clone())];
    if let Some(r) = raf {
        args.push(r.clone());
    }
    Ok(read_to_nested(it, args, Want::Scalar)?.pop().unwrap_or_default())
}

//------------------------------------------------------------------------------
// Read information in a RTF document
// Inputs: 0) ExifTool ref, 1) dirInfo ref
// Returns: 1 on success, 0 if this wasn't a valid RTF file
pub fn process_rtf(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info) = (arg(&a, 0), arg(&a, 1));
    let raf = dir_info.get("RAF");
    let mut buf2 = Value::Undef;
    let cs;

    let mut first = Value::Undef;
    if !(raf_read(it, &raf, &mut first, n(64))?.truthy() && pl::method(it, &raf, "Seek", vec![n(0), n(0)])?.truthy()) {
        return ret(n(0));
    }
    if !re(r"^[\n\r]*\{[\n\r]*\\rtf[^a-zA-Z]", "").is_match(&first) {
        return ret(n(0));
    }
    pl::method(it, &et, "SetFileType", vec![])?;
    //
    // determine the RTF character set
    //
    if let Some(m) = re(r"\\ansicpg(\d*)", "").m(&first) {
        cs = pl::concat(&s("cp"), &cap(&m, 1));
    } else if let Some(m) = re(r"\\(ansi|mac|pc|pca)[^a-zA-Z]", "").m(&first) {
        cs = match &*cap(&m, 1).to_bytes() {
            b"ansi" => s("Latin"),
            b"mac" => s("MacRoman"),
            b"pc" => s("cp437"),
            b"pca" => s("cp850"),
            _ => Value::Undef,
        };
    } else {
        pl::method(it, &et, "Warn", vec![s("Unspecified RTF encoding. Will assume Latin")])?;
        cs = s("Latin");
    }
    let charset_name = pl::pkg_hash(it, "Image::ExifTool::charsetName");
    let mut charset = charset_name.get(pl::lc(&cs).to_bytes());
    if !charset.truthy() {
        let msg = pl::concat(&pl::concat(&s("Unsupported RTF encoding "), &cs), &s(". Will assume Latin."));
        pl::method(it, &et, "Warn", vec![msg])?;
        charset = s("Latin");
    }
    let mut tag_table_ptr = pl::call(it, "Image::ExifTool::GetTagTable", vec![s("Image::ExifTool::RTF::Main")])?;
    // (my $buff: handed to ReadToNested, which uses its pos())
    let buff = cell(Value::Undef);
    let buff_pt = Value::ScalarRef(buff.clone());
    let info_re = rx(br"[^\\]\{[\n\r]*\\info([^a-zA-Z])", "")?;
    let cmd_re = rx(br"\{[\n\r]*(\\\*[\n\r]*)?\\([a-zA-Z]+)([^a-zA-Z])", "")?;
    //
    // scan for \info group
    //
    loop {
        if !raf_read(it, &raf, &mut buf2, n(65536))?.truthy() {
            break;
        }
        if !buff.borrow().is_undef() {
            // read more but leave some overlap for the match
            let v = pl::concat(&pl::substr(&buff.borrow(), -16, None), &buf2);
            assign(it, &buff_pt, v)?;
        } else {
            assign(it, &buff_pt, buf2.clone())?;
        }
        let Some((d, m)) = gmatch(it, &buff_pt, &info_re)? else { continue };
        // anything but a space is included in the contents
        if !pl::str_eq(&sp(&d, &m, 1), " ") {
            let p = pos_of(it, &buff_pt)?.unwrap_or(0);
            set_pos(it, &buff_pt, Some(p.saturating_sub(1)))?;
        }
        let info = read_to_nested_cell(it, &buff, Some(&raf))?;
        if info.is_undef() {
            pl::method(it, &et, "Warn", vec![s("Unterminated information group")])?;
            break;
        }
        // process info commands (eg. "\author", "\*\copyright");
        let info_c = cell(info);
        let info_pt = Value::ScalarRef(info_c.clone());
        while let Some((d, m)) = gmatch(it, &info_pt, &cmd_re)? {
            if !pl::str_eq(&sp(&d, &m, 3), " ") {
                let p = pos_of(it, &info_pt)?.unwrap_or(0);
                set_pos(it, &info_pt, Some(p.saturating_sub(1)))?;
            }
            let tag = sp(&d, &m, 2);
            let mut val = read_to_nested_cell(it, &info_c, None)?;
            if val.is_undef() {
                break;
            }
            let tag_info = tag_table_ptr.get(tag.to_bytes());
            if tag_info.truthy() && tag_info.get("Format").truthy() && pl::str_eq(&tag_info.get("Format"), "date") {
                // parse RTF date commands
                let idx = |k: &[u8]| -> Option<usize> {
                    match k {
                        b"yr" => Some(0),
                        b"mo" => Some(1),
                        b"dy" => Some(2),
                        b"hr" => Some(3),
                        b"min" => Some(4),
                        b"sec" => Some(5),
                        _ => None,
                    }
                };
                let mut t = vec![n(0); 6];
                for c in re(r"\\([a-z]+)(\d+)", "").each(&val) {
                    let Some(i) = idx(&cap(&c, 1).to_bytes()) else { continue };
                    t[i] = cap(&c, 2);
                }
                val = pl::sprintf("%.4d:%.2d:%.2d %.2d:%.2d:%.2d", &t);
            } else {
                // unescape RTF string value
                val = unescape_rtf(it, vec![et.clone(), val, charset.clone()], Want::Scalar)?.pop().unwrap_or_default();
            }
            // create tagInfo for unknown tags
            if !tag_info.truthy() {
                let ti = Value::new_hash();
                ti.set("Name", pl::ucfirst(&tag));
                pl::call(it, "Image::ExifTool::AddTagToTable", vec![tag_table_ptr.clone(), tag.clone(), ti])?;
            }
            pl::method(it, &et, "HandleTag", vec![tag_table_ptr.clone(), tag, val])?;
        }
    }
    if buff.borrow().is_undef() {
        return ret(n(1));
    }
    //
    // scan for \userprops (but don't read more from file to find the start of this command)
    //
    set_pos(it, &buff_pt, Some(0))?;
    let up_re = rx(br"[^\\]\{[\n\r]*\\\*[\n\r]*\\userprops([^a-zA-Z])", "")?;
    if let Some((d, m)) = gmatch(it, &buff_pt, &up_re)? {
        // Note: The RTF spec places brackets around each propinfo structure,
        // but Microsoft Word doesn't write it this way, so tolerate either.
        if !pl::str_eq(&sp(&d, &m, 1), " ") {
            let p = pos_of(it, &buff_pt)?.unwrap_or(0);
            set_pos(it, &buff_pt, Some(p.saturating_sub(1)))?;
        }
        let props = read_to_nested_cell(it, &buff, Some(&raf))?;
        tag_table_ptr = pl::call(it, "Image::ExifTool::GetTagTable", vec![s("Image::ExifTool::RTF::UserProps")])?;
        if props.is_undef() {
            pl::method(it, &et, "Warn", vec![s("Unterminated user properties")])?;
            return ret(n(1));
        }
        // process user properties
        let mut tag = Value::Undef;
        let props_c = cell(props);
        let props_pt = Value::ScalarRef(props_c.clone());
        while let Some((d, m)) = gmatch(it, &props_pt, &cmd_re)? {
            if !pl::str_eq(&sp(&d, &m, 3), " ") {
                let p = pos_of(it, &props_pt)?.unwrap_or(0);
                set_pos(it, &props_pt, Some(p.saturating_sub(1)))?;
            }
            let t = sp(&d, &m, 2);
            let mut val = read_to_nested_cell(it, &props_c, None)?;
            if val.is_undef() {
                break;
            }
            val = unescape_rtf(it, vec![et.clone(), val, charset.clone()], Want::Scalar)?.pop().unwrap_or_default();
            if pl::str_eq(&t, "propname") {
                tag = val;
                continue;
            } else if !pl::str_eq(&t, "staticval") || tag.is_undef() {
                continue; // ignore \linkval and \proptype for now
            }
            re(r"\s(.)", "").subst(&mut tag, true, |c| pl::uc(&cap(c, 1)).to_bytes().to_vec()); // capitalize all words in tag name
            pl::tr(&mut tag, "-_a-zA-Z0-9", "", "dc"); // remove illegal characters
            if !tag.truthy() {
                continue;
            }
            // create tagInfo for unknown tags
            if !tag_table_ptr.get(tag.to_bytes()).truthy() {
                let ti = Value::new_hash();
                ti.set("Name", tag.clone());
                pl::call(it, "Image::ExifTool::AddTagToTable", vec![tag_table_ptr.clone(), tag.clone(), ti])?;
            }
            pl::method(it, &et, "HandleTag", vec![tag_table_ptr.clone(), tag.clone(), val])?;
        }
        // (didn't really want to loop)
    }
    ret(n(1))
}

pub fn register(it: &mut Interp) {
    it.define_native("Image::ExifTool::RTF::ReadToNested", read_to_nested);
    it.define_native("Image::ExifTool::RTF::UnescapeRTF", unescape_rtf);
    it.define_native("Image::ExifTool::RTF::ProcessRTF", process_rtf);
}
