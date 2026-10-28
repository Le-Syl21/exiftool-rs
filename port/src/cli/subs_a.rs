//! The `exiftool` script (ExifTool 13.59), ported sub by sub (see
//! docs/port/CLI.md). This part: the small subs before the main program
//! (script lines 329-376: `Image::ExifTool::EndDir` ... `Cleanup`) and the
//! subs between SetImageInfo and ProcessFiles (lines 3678-4231:
//! `DoHardLink` ... `SetWindowTitle`).

use crate::interp::eval::ER;
use crate::interp::{Flow, Interp, Want};
use crate::perl::Value;
use crate::pl::{self, arg, cap, n, re, ret};

// ---- local helpers ----

/// The script's file lexical `$NAME` (its value).
fn sc(it: &mut Interp, name: &str) -> Value {
    super::lex_scalar(it, name).borrow().clone()
}

/// `$NAME = v` for a file lexical.
fn set_sc(it: &mut Interp, name: &str, v: Value) {
    *super::lex_scalar(it, name).borrow_mut() = v;
}

/// Perl's true and false: 1 and "".
fn pbool(b: bool) -> Value {
    if b {
        n(1)
    } else {
        pl::s("")
    }
}

/// `join($sep, LIST)` with a separator held in a value.
fn join_v(sep: &Value, items: &[Value]) -> Value {
    let sep = sep.to_bytes();
    let parts: Vec<Vec<u8>> = items.iter().map(|v| v.to_bytes().to_vec()).collect();
    Value::bytes(parts.join(&sep[..]))
}

/// Run a snippet of Perl in package main (a primitive no helper provides:
/// `\*STDERR`, `<STDIN>`, `open FH, '-'`, `printf`, `system`) with the
/// scalars `vars` bound as lexicals. `$@` is left as it was; a die or an
/// exit goes through.
fn snippet(it: &mut Interp, code: &str, vars: &[(&str, Value)], want: Want) -> ER<Vec<Value>> {
    let scope = crate::interp::Scope::new(None);
    for (name, v) in vars {
        scope.scalar(name, v.clone());
    }
    let saved_err = it.get_scalar("main::@");
    let r = it.eval_string(code.as_bytes(), "main", &scope, want);
    it.set_scalar("main::@", saved_err);
    match r {
        Ok(v) => Ok(v),
        Err(e) => match it.exit_code {
            Some(code) => Err(Flow::Exit(code)),
            None => Err(Flow::Die(e)),
        },
    }
}

/// `\*NAME` (STDOUT, STDERR, RAF_STDIN...).
fn glob_ref(it: &mut Interp, name: &str) -> ER<Value> {
    Ok(snippet(it, &format!("\\*{name}"), &[], Want::Scalar)?.pop().unwrap_or_default())
}

/// `print STDERR LIST`
fn print_stderr(it: &mut Interp, items: &[Value]) -> ER<bool> {
    let fh = glob_ref(it, "STDERR")?;
    pl::print_to(it, Some(&fh), items)
}

/// `warn(LIST)`: the items joined, perl's location added when the message
/// does not end with a newline, through `$SIG{__WARN__}` (byte-safe).
fn warn_list(it: &mut Interp, items: &[Value]) -> ER<()> {
    let mut msg: Vec<u8> = items.iter().flat_map(|v| v.to_bytes().to_vec()).collect();
    if msg.is_empty() {
        msg = b"Warning: something's wrong".to_vec();
    }
    if msg.last() != Some(&b'\n') {
        msg.extend_from_slice(crate::interp::eval::location_suffix().as_bytes());
    }
    it.warn(Value::bytes(msg))
}

/// `ref $v` as a string.
fn ref_is(v: &Value, what: &str) -> bool {
    pl::ref_of(v) == what
}

/// `keys %$h` as values.
fn keys(h: &Value) -> Vec<Value> {
    h.keys().into_iter().map(Value::Str).collect()
}

/// `sort LIST` (string order).
fn sort_str(mut v: Vec<Value>) -> Vec<Value> {
    v.sort_by(|a, b| a.to_bytes()[..].cmp(&b.to_bytes()[..]));
    v
}

/// `eval { $str =~ /$pat/ }`: Some(matched), or None when the pattern dies
/// (with `$@` set).
fn eval_match(it: &mut Interp, s: &Value, pat: &Value) -> ER<Option<bool>> {
    // (note: an empty $pat is perl's "last successful pattern"; here it
    // matches everything)
    pl::eval_block(it, |_| Ok(pl::re_dyn(&pat.to_bytes(), "")?.is_match(s)))
}

// ---- special subroutines used in -if condition (329-330) ----

/// `sub Image::ExifTool::EndDir() { return $endDir = 1 }`
pub fn end_dir(it: &mut Interp, _a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    set_sc(it, "endDir", n(1));
    ret(n(1))
}

/// `sub Image::ExifTool::End() { return $end = 1 }`
pub fn end(it: &mut Interp, _a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    set_sc(it, "end", n(1));
    ret(n(1))
}

// ---- exit routine (333-347) ----

/// exit routine
pub fn exit(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    if sc(it, "pause").truthy() {
        if pl::eval_block(it, |it| crate::runtime::require(it, "Term::ReadKey"))?.is_some() {
            print_stderr(it, &[pl::s("-- press any key --")])?;
            pl::call(it, "Term::ReadKey::ReadMode", vec![pl::s("cbreak")])?;
            pl::call(it, "Term::ReadKey::ReadKey", vec![n(0)])?;
            pl::call(it, "Term::ReadKey::ReadMode", vec![n(0)])?;
            print_stderr(it, &[pl::s(&"\x08 \x08".repeat(20))])?;
        } else {
            print_stderr(it, &[pl::s("-- press RETURN --\n")])?;
            snippet(it, "my $line = <STDIN>;", &[], Want::Void)?;
        }
    }
    // exit shift;
    // (perl warns here if the argument is undef)
    Err(Flow::Exit(arg(&a, 0).to_i64() as i32))
}

// ---- my warning and error routines (NEVER say "die"!) (349-360) ----

/// `Warn LIST`: print a warning on stderr, bypassing `$SIG{__WARN__}`
pub fn warn(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    // (perl warns here if $quiet is undef)
    if pl::lt(&sc(it, "quiet"), &n(2)) || re(r"^Error", "").is_match(&arg(&a, 0)) {
        let sig = pl::pkg_hash(it, "main::SIG");
        let old_warn = sig.get("__WARN__");
        sig.delete("__WARN__");
        pl::at(353);
        let r = warn_list(it, &a);
        if !old_warn.is_undef() {
            sig.set("__WARN__", old_warn.clone());
        }
        r?;
        // (the value of the last statement: the assignment, or "defined")
        return ret(if old_warn.is_undef() { pl::s("") } else { old_warn });
    }
    ret(pl::s(""))
}

/// `Error LIST`: warn, and set the return value to 1
pub fn error(it: &mut Interp, a: Vec<Value>, w: Want) -> ER<Vec<Value>> {
    warn(it, a, w)?;
    set_sc(it, "rtnVal", n(1));
    ret(n(1))
}

/// `WarnOnce($)`
pub fn warn_once(it: &mut Interp, a: Vec<Value>, w: Want) -> ER<Vec<Value>> {
    let warned_once = super::lex_hash(it, "warnedOnce");
    let key = arg(&a, 0).to_bytes();
    let seen = warned_once.get(&key[..]);
    if seen.truthy() {
        return ret(seen);
    }
    let r = warn(it, a, w)?.pop().unwrap_or_default();
    if r.truthy() {
        warned_once.set(&key[..], n(1));
        return ret(n(1));
    }
    ret(r)
}

// ---- signal handlers and cleanup routine (363-376) ----

/// `SigInt()`
pub fn sig_int(it: &mut Interp, _a: Vec<Value>, w: Want) -> ER<Vec<Value>> {
    if sc(it, "critical").truthy() {
        set_sc(it, "interrupted", n(1));
        return pl::ret_nothing(w);
    }
    cleanup(it, Vec::new(), Want::Void)?;
    Err(Flow::Exit(1))
}

/// `SigCont() { }`
pub fn sig_cont(_it: &mut Interp, _a: Vec<Value>, w: Want) -> ER<Vec<Value>> {
    pl::ret_nothing(w)
}

/// `Cleanup()`
pub fn cleanup(it: &mut Interp, _a: Vec<Value>, w: Want) -> ER<Vec<Value>> {
    let tmp_file = sc(it, "tmpFile");
    if !tmp_file.is_undef() {
        let mt = sc(it, "mt");
        pl::at(370);
        pl::method(it, &mt, "Unlink", vec![tmp_file])?;
    }
    let tmp_text = sc(it, "tmpText");
    if !tmp_text.is_undef() {
        let mt = sc(it, "mt");
        pl::at(371);
        pl::method(it, &mt, "Unlink", vec![tmp_text])?;
    }
    set_sc(it, "tmpFile", Value::Undef);
    set_sc(it, "tmpText", Value::Undef);
    if !super::lex_hash(it, "preserveTime").is_empty() {
        pl::at(374);
        pl::call(it, "main::PreserveTime", Vec::new())?;
    }
    set_window_title(it, vec![pl::s("")], w)
}

// ---- the subs after SetImageInfo (3678-4231) ----

/// Make hard link and handle TestName if specified
/// Inputs: 0) ExifTool ref, 1) source file name, 2) HardLink name,
///         3) SymLink name, 4) TestFile name
pub fn do_hard_link(it: &mut Interp, a: Vec<Value>, w: Want) -> ER<Vec<Value>> {
    let (et, src, mut hard_link, mut sym_link, mut test_name) = (arg(&a, 0), arg(&a, 1), arg(&a, 2), arg(&a, 3), arg(&a, 4));
    let count_link = super::lex_hash(it, "countLink");
    let or0 = |v: Value| if v.truthy() { v } else { n(0) };
    if !hard_link.is_undef() {
        hard_link = pl::call(it, "main::NextUnusedFilename", vec![hard_link])?;
        pl::at(3687);
        // (perl warns here if SetFileName returns undef)
        if pl::gt(&pl::method(it, &et, "SetFileName", vec![src.clone(), hard_link, pl::s("Link")])?, &n(0)) {
            count_link.set("Hard", pl::add(&or0(count_link.get("Hard")), &n(1)));
        } else {
            count_link.set("BadHard", pl::add(&or0(count_link.get("BadHard")), &n(1)));
        }
    }
    if !sym_link.is_undef() {
        sym_link = pl::call(it, "main::NextUnusedFilename", vec![sym_link])?;
        pl::at(3695);
        if pl::gt(&pl::method(it, &et, "SetFileName", vec![src.clone(), sym_link, pl::s("SymLink")])?, &n(0)) {
            count_link.set("Sym", pl::add(&or0(count_link.get("Sym")), &n(1)));
        } else {
            count_link.set("BadSym", pl::add(&or0(count_link.get("BadSym")), &n(1)));
        }
    }
    if !test_name.is_undef() {
        test_name = pl::call(it, "main::NextUnusedFilename", vec![test_name, src.clone()])?;
        let used_file_name = super::lex_hash(it, "usedFileName");
        let tn = test_name.to_bytes();
        if used_file_name.get(&tn[..]).truthy() {
            pl::at(3704);
            pl::method(it, &et, "Warn", vec![pl::concat(&pl::concat(&pl::s("File '"), &test_name), &pl::s("' would exist"))])?;
        } else {
            pl::at(3705);
            let used = used_file_name.get(&tn[..]);
            // (perl warns here if SetFileName returns undef)
            if pl::num_eq(&pl::method(it, &et, "SetFileName", vec![src.clone(), test_name.clone(), pl::s("Test"), used])?, &n(1)) {
                used_file_name.set(&tn[..], n(1));
                used_file_name.set(&src.to_bytes()[..], n(0));
            }
        }
    }
    pl::ret_nothing(w)
}

/// Clean string for XML (also removes invalid control chars and malformed UTF-8)
/// Inputs: 0) string ref
/// Returns: nothing, but input string is escaped
pub fn clean_xml(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let str_pt = arg(&a, 0);
    // translate control characters that are invalid in XML
    pl::at(3719);
    let mut s = pl::sderef(&str_pt)?;
    pl::tr(&mut s, r"\0-\x08\x0b\x0c\x0e-\x1f", ".", "");
    pl::set_deref(&str_pt, s)?;
    // fix malformed UTF-8 characters
    if !sc(it, "altEnc").truthy() {
        pl::call(it, "Image::ExifTool::XMP::FixUTF8", vec![str_pt.clone()])?;
    }
    // escape necessary characters for XML
    let s = pl::sderef(&str_pt)?;
    let v = pl::call(it, "Image::ExifTool::XMP::EscapeXML", vec![s])?;
    pl::set_deref(&str_pt, v.clone())?;
    ret(v)
}

/// Encode string for XML
/// Inputs: 0) string ref
/// Returns: encoding used (and input string is translated)
pub fn encode_xml(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let str_pt = arg(&a, 0);
    pl::at(3734);
    let s = pl::sderef(&str_pt)?;
    if re(r"[\x00-\x08\x0b\x0c\x0e-\x1f]", "").is_match(&s)
        || (!sc(it, "altEnc").truthy() && pl::lt(&pl::call(it, "Image::ExifTool::IsUTF8", vec![str_pt.clone()])?, &n(0)))
    {
        // encode binary data and non-UTF8 with special characters as base64
        let s = pl::sderef(&str_pt)?;
        let v = pl::call(it, "Image::ExifTool::XMP::EncodeBase64", vec![s])?;
        pl::set_deref(&str_pt, v)?;
        // #ATV = Alexander Vonk, private communication
        return ret(pl::s("http://www.w3.org/2001/XMLSchema#base64Binary")); //ATV
    } else if sc(it, "escapeHTML").truthy() {
        let s = pl::sderef(&str_pt)?;
        let alt_enc = sc(it, "altEnc");
        let v = pl::call(it, "Image::ExifTool::HTML::EscapeHTML", vec![s, alt_enc])?;
        pl::set_deref(&str_pt, v)?;
    } else {
        let s = pl::sderef(&str_pt)?;
        let v = pl::call(it, "Image::ExifTool::XMP::EscapeXML", vec![s])?;
        pl::set_deref(&str_pt, v)?;
    }
    ret(pl::s("")) // not encoded
}

/// Format value for XML output
/// Inputs: 0) value, 1) indentation, 2) group
/// Returns: formatted value
pub fn format_xml(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    pl::local_topic(it, |it| {
        let (mut val, ind, grp) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
        let mut gt = pl::s(">");
        if ref_is(&val, "ARRAY") {
            // convert ARRAY into an rdf:Bag
            let mut val2 = pl::concat(&pl::concat(&pl::s("\n"), &ind), &pl::s(" <rdf:Bag>"));
            let sub_ind = pl::concat(&ind, &pl::s("  "));
            for item in val.items() {
                it.set_scalar("main::_", item.clone());
                let f = format_xml(it, vec![item, sub_ind.clone(), grp.clone()], Want::Scalar)?.pop().unwrap_or_default();
                val2 = pl::concat(&val2, &pl::concat(&pl::concat(&pl::concat(&pl::concat(&pl::s("\n"), &ind), &pl::s("  <rdf:li")), &f), &pl::s("</rdf:li>")));
            }
            val = pl::concat(&pl::concat(&pl::concat(&pl::concat(&val2, &pl::s("\n")), &ind), &pl::s(" </rdf:Bag>\n")), &ind);
        } else if ref_is(&val, "HASH") {
            gt = pl::s(" rdf:parseType='Resource'>");
            let mut val2 = pl::s("");
            for key in pl::call_list(it, "main::OrderedKeys", vec![val.clone()])? {
                it.set_scalar("main::_", key.clone());
                // (some variable-namespace XML structure fields may have a different group)
                let (mut ns, mut tg) = (grp.clone(), key.clone());
                if let Some(m) = re(r"^(.*?):(.*)", "").m(&key) {
                    if pl::str_eq(&grp, "JSON") {
                        pl::tr(&mut tg, ":", "_", ""); // colons in JSON structure elements are not namespaces
                    } else {
                        (ns, tg) = (cap(&m, 1), cap(&m, 2));
                    }
                }
                // validate XML attribute name
                for name in [&mut ns, &mut tg] {
                    // make sure name is valid for XML
                    pl::tr(name, "-_A-Za-z0-9.", "", "dc");
                    if !re(r"^[_A-Za-z]", "").is_match(name) {
                        *name = pl::concat(&pl::s("_"), name);
                    }
                }
                let tok = pl::concat(&pl::concat(&ns, &pl::s(":")), &tg);
                let sub_ind = pl::concat(&ind, &pl::s(" "));
                let f = format_xml(it, vec![val.get(&key.to_bytes()[..]), sub_ind, grp.clone()], Want::Scalar)?.pop().unwrap_or_default();
                let mut piece = pl::concat(&pl::concat(&pl::concat(&pl::s("\n"), &ind), &pl::s(" <")), &tok);
                piece = pl::concat(&pl::concat(&pl::concat(&piece, &f), &pl::s("</")), &pl::concat(&tok, &pl::s(">")));
                val2 = pl::concat(&val2, &piece);
            }
            val = pl::concat(&pl::concat(&val2, &pl::s("\n")), &ind);
        } else {
            // (note: SCALAR reference should have already been converted)
            let r = pl::sref(val.clone());
            let enc = encode_xml(it, vec![r.clone()], Want::Scalar)?.pop().unwrap_or_default();
            val = r.deref();
            if enc.truthy() {
                gt = pl::concat(&pl::concat(&pl::s(" rdf:datatype='"), &enc), &pl::s("'>\n")); //ATV
            }
        }
        ret(pl::concat(&gt, &val))
    })
}

/// Escape string for JSON or PHP
/// Inputs: 0) string, 1) flag to force numbers to be quoted too
/// Returns: Escaped string (quoted if necessary)
pub fn escape_json(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (mut s, quote) = (arg(&a, 0), arg(&a, 1));
    let json = sc(it, "json");
    // (perl warns here if $str or $json is undef)
    if !quote.truthy() {
        // JSON boolean (true or false)
        if re(r"^(true|false)$", "i").is_match(&s) && pl::lt(&json, &n(2)) {
            return ret(pl::lc(&s));
        }
        // JSON/PHP number (see json.org for numerical format)
        // return $str if $str =~ /^-?(\d|[1-9]\d+)(\.\d+)?(e[-+]?\d+)?$/i;
        // (these big numbers caused problems for some JSON parsers, so be more conservative)
        if re(r"^-?(\d|[1-9]\d{1,14})(\.\d{1,16})?(e[-+]?\d{1,3})?$", "i").is_match(&s) {
            return ret(s);
        }
    }
    // encode JSON string in base64 if necessary
    if pl::lt(&json, &n(2))
        && !sc(it, "binaryOutput").is_undef()
        && pl::lt(&pl::call(it, "Image::ExifTool::IsUTF8", vec![pl::sref(s.clone())])?, &n(0))
    {
        let b = pl::call(it, "Image::ExifTool::XMP::EncodeBase64", vec![s, n(1)])?;
        return ret(pl::concat(&pl::concat(&pl::s("\"base64:"), &b), &pl::s("\"")));
    }
    // escape special characters
    let json_char = super::lex_hash(it, "jsonChar");
    re(r#"(["\t\n\r\\])"#, "s").subst(&mut s, true, |c| {
        let mut o = b"\\".to_vec();
        o.extend_from_slice(&json_char.get(&cap(c, 1).to_bytes()[..]).to_bytes());
        o
    });
    if pl::lt(&json, &n(2)) {
        // JSON
        pl::tr(&mut s, r"\0", "", "d"); // remove all nulls
        // escape other control characters with \u
        re(r"([\x00-\x1f\x7f])", "s").subst(&mut s, true, |c| pl::sprintf("\\u%.4X", &[n(cap(c, 1).to_bytes()[0] as i64)]).to_bytes().to_vec());
        // JSON strings must be valid UTF8
        if !sc(it, "altEnc").truthy() {
            let r = pl::sref(s.clone());
            pl::call(it, "Image::ExifTool::XMP::FixUTF8", vec![r.clone()])?;
            s = r.deref();
        }
    } else {
        // PHP
        if !sc(it, "isBinary").truthy() {
            re(r"\x00+$", "").subst(&mut s, false, |_| Vec::new()); // remove trailing nulls unless binary
        }
        // must escape "$" too for PHP
        re(r"\$", "s").subst(&mut s, true, |_| b"\\$".to_vec());
        // escape other control characters with \x
        re(r"([\x00-\x1f\x7f])", "s").subst(&mut s, true, |c| pl::sprintf("\\x%.2X", &[n(cap(c, 1).to_bytes()[0] as i64)]).to_bytes().to_vec());
    }
    ret(pl::concat(&pl::concat(&pl::s("\""), &s), &pl::s("\""))) // return the quoted string
}

/// Print JSON or PHP value
/// Inputs: 0) file reference, 1) value, 2) indentation, 3) true to quote numbers
pub fn format_json(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    pl::local_topic(it, |it| {
        let (fp, val, ind, quote) = (arg(&a, 0), arg(&a, 1), arg(&a, 2), arg(&a, 3));
        let mut comma = false;
        let json = sc(it, "json");
        let last;
        if !val.is_ref() {
            let e = escape_json(it, vec![val, quote], Want::List)?;
            last = pbool(pl::print_to(it, Some(&fp), &e)?);
        } else if ref_is(&val, "ARRAY") {
            if sc(it, "joinLists").truthy() && !val.at(0).is_ref() {
                let list_sep = sc(it, "listSep");
                let j = join_v(&list_sep, &val.items());
                let e = escape_json(it, vec![j, quote], Want::List)?;
                last = pbool(pl::print_to(it, Some(&fp), &e)?);
            } else {
                let (bra, ket) = if pl::num_eq(&json, &n(1)) { ("[", "]") } else { ("Array(", ")") };
                pl::print_to(it, Some(&fp), &[pl::s(bra)])?;
                for item in val.items() {
                    it.set_scalar("main::_", item.clone());
                    if comma {
                        pl::print_to(it, Some(&fp), &[pl::s(",")])?;
                    }
                    format_json(it, vec![fp.clone(), item, ind.clone(), quote.clone()], Want::Void)?;
                    comma = true;
                }
                last = pbool(pl::print_to(it, Some(&fp), &[pl::s(ket)])?);
            }
        } else if ref_is(&val, "HASH") {
            let (bra, ket, sep) = if pl::num_eq(&json, &n(1)) { ("{", "}", ":") } else { ("Array(", ")", " =>") };
            pl::print_to(it, Some(&fp), &[pl::s(bra)])?;
            let sub_ind = pl::concat(&ind, &pl::s("  "));
            for k in pl::call_list(it, "main::OrderedKeys", vec![val.clone()])? {
                it.set_scalar("main::_", k.clone());
                if comma {
                    pl::print_to(it, Some(&fp), &[pl::s(",")])?;
                }
                let key = escape_json(it, vec![k.clone(), n(1)], Want::Scalar)?.pop().unwrap_or_default();
                let line = pl::concat(&pl::concat(&pl::concat(&pl::concat(&pl::s("\n"), &sub_ind), &key), &pl::s(sep)), &pl::s(" "));
                pl::print_to(it, Some(&fp), &[line])?;
                let v = val.get(&k.to_bytes()[..]);
                // hack to force decimal id's to be printed as strings with -H
                let show_tag_id = sc(it, "showTagID");
                if show_tag_id.truthy() && pl::str_eq(&k, "id") && pl::str_eq(&show_tag_id, "H") && re(r"^\d+\.\d+$", "").is_match(&v) {
                    pl::print_to(it, Some(&fp), &[pl::concat(&pl::concat(&pl::s("\""), &v), &pl::s("\""))])?;
                } else {
                    format_json(it, vec![fp.clone(), v, sub_ind.clone(), quote.clone()], Want::Void)?;
                }
                comma = true;
            }
            last = pbool(pl::print_to(it, Some(&fp), &[pl::concat(&pl::concat(&pl::s("\n"), &ind), &pl::s(ket))])?);
        } else {
            // (note: SCALAR reference should have already been converted)
            last = pbool(pl::print_to(it, Some(&fp), &[pl::s("\"<err>\"")])?);
        }
        ret(last)
    })
}

/// Format value for CSV file
/// Inputs: value
/// Returns: value quoted if necessary
pub fn format_csv(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let mut val = arg(&a, 0);
    // check for valid encoding if the Charset option was used
    let set_charset = sc(it, "setCharset");
    if set_charset.truthy()
        && (re(r"[^\x09\x0a\x0d\x20-\x7e\x80-\xff]", "").is_match(&val)
            || (pl::str_eq(&set_charset, "UTF8") && pl::lt(&pl::call(it, "Image::ExifTool::IsUTF8", vec![pl::sref(val.clone())])?, &n(0))))
    {
        let b = pl::call(it, "Image::ExifTool::XMP::EncodeBase64", vec![val, n(1)])?;
        val = pl::concat(&pl::s("base64:"), &b);
    }
    // currently, there is a chance that the value may contain NULL characters unless
    // the -b option is used to encode as Base64.  It is unclear whether or not this
    // is valid CSV, but some readers may not like it.  (If this becomes a problem,
    // in the future values may need to be truncated at the first NULL character.)
    let csv_delim = sc(it, "csvDelim");
    let quote = re("\"", "").subst(&mut val, true, |_| b"\"\"".to_vec()) > 0 || re(r"(^\s+|\s+$)", "").is_match(&val) || {
        let mut p = br"[\n\r]|".to_vec();
        p.extend_from_slice(&crate::perl::regex::quotemeta(&csv_delim.to_bytes()));
        pl::at(3898);
        pl::re_dyn(&p, "")?.is_match(&val)
    };
    if quote {
        val = pl::concat(&pl::concat(&pl::s("\""), &val), &pl::s("\""));
    }
    ret(val)
}

/// Print accumulated CSV information
/// Inputs: 0) file ref
pub fn print_csv(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let a0 = arg(&a, 0);
    let fp = if a0.truthy() { a0 } else { glob_ref(it, "STDOUT")? };
    let mut tags: Vec<Value> = Vec::new();

    let csv_tags_a = super::lex_array(it, "csvTags");
    let csv_tags_h = super::lex_hash(it, "csvTags");
    if csv_tags_a.is_empty() {
        let sorted = sort_str(keys(&csv_tags_h));
        if let Value::ArrayRef(r) = &csv_tags_a {
            *r.borrow_mut() = sorted;
        }
    }
    // make a list of tags actually found
    for lc_tag in csv_tags_a.items() {
        let t = csv_tags_h.get(&lc_tag.to_bytes()[..]);
        if t.truthy() {
            tags.push(format_csv(it, vec![t], Want::Scalar)?.pop().unwrap_or_default());
        }
    }
    let csv_delim = sc(it, "csvDelim");
    let mut head = vec![pl::s("SourceFile")];
    head.extend(tags);
    let line = join_v(&csv_delim, &head);
    pl::print_to(it, Some(&fp), &[line, pl::s("\n")])?;
    let force_print = sc(it, "forcePrint");
    let empty = if !force_print.is_undef() { force_print } else { pl::s("") };
    let database = super::lex_hash(it, "database");
    let mut last = false;
    for file in super::lex_array(it, "csvFiles").items() {
        let mut vals = vec![format_csv(it, vec![file.clone()], Want::Scalar)?.pop().unwrap_or_default()]; // start with full file name
        let csv_info = database.get(&file.to_bytes()[..]);
        for lc_tag in csv_tags_a.items() {
            let k = lc_tag.to_bytes();
            if !csv_tags_h.get(&k[..]).truthy() {
                continue;
            }
            let val = csv_info.get(&k[..]);
            if val.is_undef() {
                vals.push(empty.clone());
                continue;
            }
            vals.push(format_csv(it, vec![val], Want::Scalar)?.pop().unwrap_or_default());
        }
        let line = join_v(&csv_delim, &vals);
        last = pl::print_to(it, Some(&fp), &[line, pl::s("\n")])?;
    }
    let _ = last;
    pl::ret_nothing(Want::Scalar)
}

/// Add tag groups from structure fields to a list for xmlns
/// Inputs: 0) tag value, 1) parent group, 2) group hash ref, 3) group list ref
pub fn add_groups(it: &mut Interp, a: Vec<Value>, w: Want) -> ER<Vec<Value>> {
    let (val, grp, group_hash, group_list) = (arg(&a, 0), arg(&a, 1), arg(&a, 2), arg(&a, 3));
    if ref_is(&val, "HASH") {
        for key in sort_str(keys(&val)) {
            if let Some(m) = re(r"^(.*?):", "").m(&key) {
                let g = cap(&m, 1);
                if !group_hash.get(&g.to_bytes()[..]).truthy() && !pl::str_eq(&grp, "JSON") {
                    group_hash.set(&g.to_bytes()[..], grp.clone());
                    group_list.push(g);
                }
            }
            let v = val.get(&key.to_bytes()[..]);
            if v.is_ref() {
                add_groups(it, vec![v, grp.clone(), group_hash.clone(), group_list.clone()], Want::Void)?;
            }
        }
    } else if ref_is(&val, "ARRAY") {
        for val2 in val.items() {
            if val2.is_ref() {
                add_groups(it, vec![val2, grp.clone(), group_hash.clone(), group_list.clone()], Want::Void)?;
            }
        }
    }
    pl::ret_nothing(w)
}

/// Convert binary data (SCALAR references) for printing
/// Inputs: 0) object reference
/// Returns: converted object, or undef if we don't want binary objects
pub fn convert_binary(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let mut obj = arg(&a, 0);
    if ref_is(&obj, "HASH") {
        for key in keys(&obj) {
            let k = key.to_bytes();
            let v = obj.get(&k[..]);
            if !v.is_ref() {
                continue;
            }
            let c = convert_binary(it, vec![v], Want::Scalar)?.pop().unwrap_or_default();
            obj.set(&k[..], c.clone());
            if c.is_undef() {
                return pl::ret_undef();
            }
        }
    } else if ref_is(&obj, "ARRAY") {
        for i in 0..obj.len() {
            let v = obj.at(i as i64);
            if !v.is_ref() {
                continue;
            }
            let c = convert_binary(it, vec![v], Want::Scalar)?.pop().unwrap_or_default();
            obj.set_at(i, c.clone());
            if c.is_undef() {
                return pl::ret_undef();
            }
        }
    } else if ref_is(&obj, "SCALAR") {
        if sc(it, "noBinary").truthy() {
            return pl::ret_undef();
        }
        // (binaryOutput flag is set to 0 for binary mode of XML/PHP/JSON output formats)
        if !sc(it, "binaryOutput").is_undef() {
            obj = obj.deref();
            // encode in base64 if necessary (0xf7 allows for up to 21-bit UTF-8 code space)
            if pl::num_eq(&sc(it, "json"), &n(1))
                && (re(r"[^\x09\x0a\x0d\x20-\x7e\x80-\xf7]", "").is_match(&obj)
                    || pl::lt(&pl::call(it, "Image::ExifTool::IsUTF8", vec![pl::sref(obj.clone())])?, &n(0)))
            {
                let b = pl::call(it, "Image::ExifTool::XMP::EncodeBase64", vec![obj, n(1)])?;
                obj = pl::concat(&pl::s("base64:"), &b);
            }
        } else {
            // (-b is not valid for HTML output)
            let b_opt = if sc(it, "html").truthy() { pl::s("") } else { pl::s(", use -b option to extract") };
            let d = obj.deref();
            if re(r"^Binary data \d+ bytes$", "").is_match(&d) {
                obj = pl::concat(&pl::concat(&pl::concat(&pl::s("("), &d), &b_opt), &pl::s(")"));
            } else {
                obj = pl::concat(&pl::concat(&pl::concat(&pl::s("(Binary data "), &pl::length(&d)), &pl::s(" bytes")), &pl::concat(&b_opt, &pl::s(")")));
            }
        }
    }
    ret(obj)
}

/// Compare ValueConv and PrintConv values of a tag to see if they are equal
/// Inputs: 0) value1, 1) value2, 2) flag to return true for any scalar references
/// Returns: true if they are equal
pub fn is_equal(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (va, vb, true_scalar) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    // (scalar values are not print-converted)
    // (perl warns here if a value is undef)
    if va.to_bytes() == vb.to_bytes() {
        return ret(n(1));
    }
    if ref_is(&va, "SCALAR") {
        if true_scalar.truthy() {
            return ret(n(1));
        }
        if ref_is(&vb, "SCALAR") && va.deref().to_bytes() == vb.deref().to_bytes() {
            return ret(n(1));
        }
        return ret(n(0));
    }
    if ref_is(&va, "HASH") && ref_is(&vb, "HASH") {
        if va.len() != vb.len() {
            return ret(n(0));
        }
        for key in keys(&va) {
            let k = key.to_bytes();
            let r = is_equal(it, vec![va.get(&k[..]), vb.get(&k[..]), true_scalar.clone()], Want::Scalar)?.pop().unwrap_or_default();
            if !r.truthy() {
                return ret(n(0));
            }
        }
    } else {
        if !ref_is(&va, "ARRAY") || !ref_is(&vb, "ARRAY") || va.len() != vb.len() {
            return ret(n(0));
        }
        for i in 0..va.len() {
            let r = is_equal(it, vec![va.at(i as i64), vb.at(i as i64), true_scalar.clone()], Want::Scalar)?.pop().unwrap_or_default();
            if !r.truthy() {
                return ret(n(0));
            }
        }
    }
    ret(n(1))
}

/// Get the printable rendition of a value
/// Inputs: 0) value (may be a reference)
/// Returns: de-referenced value
pub fn printable(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let mut val = arg(&a, 0);
    if val.is_ref() {
        if sc(it, "structOpt").truthy() {
            crate::runtime::require(it, "Image::ExifTool::XMP")?;
            let mt = sc(it, "mt");
            val = pl::call(it, "Image::ExifTool::XMP::SerializeStruct", vec![mt, val])?;
        } else if ref_is(&val, "ARRAY") {
            val = join_v(&sc(it, "listSep"), &val.items());
        } else if ref_is(&val, "SCALAR") {
            val = pl::concat(&pl::concat(&pl::s("(Binary data "), &pl::length(&val.deref())), &pl::s(" bytes)"));
        }
    }
    if sc(it, "escapeC").truthy() {
        let esc_c = super::lex_hash(it, "escC");
        re(r"([\x00-\x1f\\\x7f])", "").subst(&mut val, true, |c| {
            let ch = cap(c, 1);
            let e = esc_c.get(&ch.to_bytes()[..]);
            if e.truthy() {
                e.to_bytes().to_vec()
            } else {
                pl::sprintf("\\x%.2x", &[n(ch.to_bytes()[0] as i64)]).to_bytes().to_vec()
            }
        });
    } else {
        // translate unprintable chars in value and remove trailing spaces
        pl::tr(&mut val, r"\x01-\x1f\x7f", ".", "");
        re(r"\x00", "").subst(&mut val, true, |_| Vec::new());
        re(r"\s+$", "").subst(&mut val, false, |_| Vec::new());
    }
    ret(val)
}

/// Get character length of a UTF-8 string
/// Inputs: 0) string
/// Returns: number of characters (not bytes) in the UTF-8 string
pub fn length_utf8(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let mut s = arg(&a, 0);
    if !sc(it, "fixLen").truthy() {
        return ret(pl::length(&s));
    }
    let nop = snippet(it, "sub { }", &[], Want::Scalar)?.pop().unwrap_or_default();
    pl::local_sig_warn(it, nop, |it| {
        let mt = sc(it, "mt");
        if !mt.path(&["OPTIONS", "EncodeHangs"]).truthy() && pl::eval_block(it, |it| crate::runtime::require(it, "Encode"))?.is_some() {
            s = pl::call(it, "Encode::decode_utf8", vec![s.clone()])?;
        } else {
            let c = pl::unpack("C*", &s)?;
            s = pl::pack("U0C*", &c)?;
        }
        let len;
        if pl::num_eq(&sc(it, "fixLen"), &n(1)) {
            len = pl::length(&s);
        } else {
            let gcstr = pl::eval_block(it, |it| pl::method(it, &pl::s("Unicode::GCString"), "new", vec![s.clone()]))?.unwrap_or_default();
            if gcstr.truthy() {
                len = pl::method(it, &gcstr, "columns", Vec::new())?;
            } else {
                len = pl::length(&s);
                pl::pkg_hash(it, "main::SIG").delete("__WARN__");
                let mt = sc(it, "mt");
                warning(it, vec![mt, pl::s("Unicode::GCString problem.  Columns may be misaligned")], Want::Void)?;
                set_sc(it, "fixLen", n(1));
            }
        }
        ret(len)
    })
}

/// Add tag list for copying tags from specified file
/// Inputs: 0) set tags file name (or FMT), 1) options for SetNewValuesFromFile()
/// Returns: nothing
/// Notes: Uses global variables: %setTags, %setTagsList, @newValues, $saveCount
pub fn add_set_tags_file(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (set_file, mut opts) = (arg(&a, 0), arg(&a, 1));
    let k = set_file.to_bytes();
    let set_tags = super::lex_hash(it, "setTags");
    let set_tags_list = super::lex_hash(it, "setTagsList");
    if set_tags.get(&k[..]).truthy() {
        // move these tags aside and make a new list for the next invocation of this file
        if !set_tags_list.get(&k[..]).truthy() {
            set_tags_list.set(&k[..], Value::new_array(Vec::new()));
        }
        set_tags_list.get_or_new_array(&k[..]).push(set_tags.get(&k[..]));
    }
    set_tags.set(&k[..], Value::new_array(Vec::new())); // create list for tags to copy from this file
    // insert marker to save new values now (necessary even if this is not a dynamic
    // file in case the same file is source'd multiple times in a single command)
    let save_count = pl::add(&sc(it, "saveCount"), &n(1));
    set_sc(it, "saveCount", save_count.clone());
    let marker = Value::new_hash();
    marker.set("SaveCount", save_count.clone());
    let new_values = super::lex_array(it, "newValues");
    new_values.push(marker);
    new_values.push(pl::concat(&pl::s("TagsFromFile="), &set_file));
    // add option to protect the tags which are assigned after this
    // (this is the mechanism by which the command-line order-of-operations is preserved)
    if !opts.truthy() {
        opts = Value::new_hash();
    }
    pl::at(4107);
    pl::hderef(&opts)?.set("ProtectSaved", save_count);
    let list = set_tags.get_or_new_array(&k[..]);
    list.push(opts);
    ret(n(list.len() as i64))
}

/// Get input file name or reference for calls to the ExifTool API
/// Inputs: 0) file name ('-' for STDIN), 1) flag to buffer STDIN
/// Returns: file name, or RAF reference for buffering STDIN
pub fn infile(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (file, buffer_stdin) = (arg(&a, 0), arg(&a, 1));
    if pl::str_eq(&file, "-") && (buffer_stdin.truthy() || sc(it, "rafStdin").truthy()) {
        let raf_stdin = sc(it, "rafStdin");
        if raf_stdin.truthy() {
            pl::method(it, &raf_stdin, "Seek", vec![n(0)])?; // rewind
        } else if snippet(it, "open RAF_STDIN, '-'", &[], Want::Scalar)?.pop().unwrap_or_default().truthy() {
            let fh = glob_ref(it, "RAF_STDIN")?;
            let raf = pl::method(it, &pl::s("File::RandomAccess"), "new", vec![fh])?;
            set_sc(it, "rafStdin", raf.clone());
            pl::method(it, &raf, "BinMode", Vec::new())?;
        }
        let raf_stdin = sc(it, "rafStdin");
        if raf_stdin.truthy() {
            return ret(raf_stdin);
        }
    }
    ret(file)
}

/// Issue warning to stderr, adding leading "Warning: " and trailing newline
/// if the warning isn't suppressed by the API NoWarning option
/// Inputs: 0) ExifTool ref, 1) warning string
pub fn warning(it: &mut Interp, a: Vec<Value>, w: Want) -> ER<Vec<Value>> {
    let (et, s) = (arg(&a, 0), arg(&a, 1));
    pl::at(4133);
    let no_warn = pl::method(it, &et, "Options", vec![pl::s("NoWarning")])?;
    if no_warn.is_undef() || eval_match(it, &s, &no_warn)? != Some(true) {
        let msg = pl::concat(&pl::concat(&pl::s("Warning: "), &s), &pl::s("\n"));
        return warn(it, vec![msg], w);
    }
    ret(pl::s(""))
}

/// Set new values from file
/// Inputs: 0) ExifTool ref, 1) filename, 2) reference to list of values to set
/// Returns: 0 on error (and increments $countBadWr)
pub fn do_set_from_file(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    pl::local_topic(it, |it| {
        let (et, file, set_tags) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
        if sc(it, "verbose").truthy() {
            let vout = sc(it, "vout");
            pl::at(4147);
            pl::print_to(it, Some(&vout), &[pl::concat(&pl::concat(&pl::s("Setting new values from "), &file), &pl::s("\n"))])?;
        }
        let inf = infile(it, vec![file.clone(), n(1)], Want::Scalar)?.pop().unwrap_or_default();
        let mut args = vec![inf];
        args.extend(set_tags.items());
        pl::at(4148);
        let info = pl::method(it, &et, "SetNewValuesFromFile", args)?;
        let mut num_set = n(info.len() as i64);
        if info.get("Error").truthy() {
            // delete all error and warning tags
            let warns: Vec<Value> = keys(&info).into_iter().filter(|k| re(r"^(Error|Warning)\b", "").is_match(k)).collect();
            num_set = pl::sub(&num_set, &n(warns.len() as i64));
            // issue a warning only for the main error
            let err = info.get("Error");
            for k in &warns {
                info.delete(&k.to_bytes()[..]);
            }
            pl::at(4159);
            let no_warn = pl::method(it, &et, "Options", vec![pl::s("NoWarning")])?;
            if !(!no_warn.is_undef() && eval_match(it, &err, &no_warn)? == Some(true)) {
                info.set("Warning", err);
            }
        } else if info.get("Warning").truthy() {
            let mut warns = n(1);
            while info.get(&pl::concat(&pl::concat(&pl::s("Warning ("), &warns), &pl::s(")")).to_bytes()[..]).truthy() {
                warns = pl::add(&warns, &n(1));
            }
            num_set = pl::sub(&num_set, &warns);
        }
        pl::at(4167);
        if pl::call(it, "main::PrintErrors", vec![et.clone(), info, file.clone()])?.truthy() {
            pl::call(it, "main::EFile", vec![file])?;
            let c = pl::add(&sc(it, "countBadWr"), &n(1));
            set_sc(it, "countBadWr", c);
            return ret(n(0));
        }
        if !num_set.truthy() {
            let msg = pl::concat(&pl::s("No writable tags set from "), &file);
            warning(it, vec![et, msg], Want::Void)?;
        }
        ret(n(1))
    })
}

/// Translate backslashes to forward slashes in filename if necessary
/// Inputs: 0) Filename
/// Returns: nothing, but changes filename if necessary
pub fn clean_filename(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let is_pc = pl::call(it, "Image::ExifTool::IsPC", Vec::new())?;
    if is_pc.truthy() {
        let mut f = arg(&a, 0);
        let c = pl::tr(&mut f, r"\\", "/", "");
        it.set_arg(0, f)?;
        return ret(c);
    }
    ret(is_pc)
}

/// Does path name contain wildcards
/// Inputs: 0) path name
/// Returns: true if path contains wildcards
pub fn has_wildcards(it: &mut Interp, a: Vec<Value>, w: Want) -> ER<Vec<Value>> {
    let path = arg(&a, 0);

    // if this is a Windows path with the long path prefix, then wildcards are not supported
    if pl::str_eq(&it.get_scalar("main::^O"), "MSWin32") && re(r"^[\\/]{2}\?[\\/]", "").is_match(&path) {
        return ret(n(0));
    }
    if re(r"[*?]", "").is_match(&path) {
        ret(n(1))
    } else if w == Want::List {
        Ok(Vec::new())
    } else {
        ret(pl::s(""))
    }
}

/// Check for valid UTF-8 of a file name
/// Inputs: 0) string, 1) original encoding
/// Returns: 0=plain ASCII, 1=valid UTF-8, -1=invalid UTF-8 (and print warning)
pub fn check_utf8(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (file, enc) = (arg(&a, 0), arg(&a, 1));
    let mut is_utf8 = n(0);
    if re(r"[\x80-\xff]", "").is_match(&file) {
        is_utf8 = pl::call(it, "Image::ExifTool::IsUTF8", vec![pl::sref(file.clone())])?;
        if pl::lt(&is_utf8, &n(0)) {
            if enc.truthy() {
                let msg = pl::concat(&pl::concat(&pl::s("Invalid filename encoding for "), &file), &pl::s("\n"));
                warn(it, vec![msg], Want::Void)?;
            } else if enc.is_undef() {
                warn_once(it, vec![pl::s("FileName encoding not specified.  Use \"-charset FileName=CHARSET\"\n")], Want::Void)?;
            }
        }
    }
    ret(is_utf8)
}

/// Set window title
/// Inputs: title string or '' to reset title
pub fn set_window_title(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let mut title = arg(&a, 0);
    // (perl warns here if $title is undef)
    if sc(it, "curTitle").to_bytes() != title.to_bytes() {
        set_sc(it, "curTitle", title.clone());
        if pl::str_eq(&it.get_scalar("main::^O"), "MSWin32") {
            // allow only safe characters
            pl::tr(&mut title, r"-_a-zA-Z0-9 \(\)[]{}%.+/:;,=?*!@#$~'", "", "dc");
            let r = pl::eval_block(it, |it| snippet(it, "system qq{title $title}", &[("title", title.clone())], Want::Scalar))?;
            return ret(r.and_then(|mut v| v.pop()).unwrap_or_default());
        } else {
            // (this only works for XTerm terminals, and STDERR must go to the console)
            let r = snippet(it, "printf STDERR \"\\033]0;%s\\007\", $title", &[("title", title)], Want::Scalar)?;
            return ret(r.last().cloned().unwrap_or_default());
        }
    }
    ret(pl::s(""))
}

pub fn register(it: &mut Interp) {
    super::define(it, "Image::ExifTool::EndDir", end_dir);
    super::define(it, "Image::ExifTool::End", end);
    super::define(it, "main::Exit", exit);
    super::define(it, "main::Warn", warn);
    super::define(it, "main::Error", error);
    super::define(it, "main::WarnOnce", warn_once);
    super::define(it, "main::SigInt", sig_int);
    super::define(it, "main::SigCont", sig_cont);
    super::define(it, "main::Cleanup", cleanup);
    super::define(it, "main::DoHardLink", do_hard_link);
    super::define(it, "main::CleanXML", clean_xml);
    super::define(it, "main::EncodeXML", encode_xml);
    super::define(it, "main::FormatXML", format_xml);
    super::define(it, "main::EscapeJSON", escape_json);
    super::define(it, "main::FormatJSON", format_json);
    super::define(it, "main::FormatCSV", format_csv);
    super::define(it, "main::PrintCSV", print_csv);
    super::define(it, "main::AddGroups", add_groups);
    super::define(it, "main::ConvertBinary", convert_binary);
    super::define(it, "main::IsEqual", is_equal);
    super::define(it, "main::Printable", printable);
    super::define(it, "main::LengthUTF8", length_utf8);
    super::define(it, "main::AddSetTagsFile", add_set_tags_file);
    super::define(it, "main::Infile", infile);
    super::define(it, "main::Warning", warning);
    super::define(it, "main::DoSetFromFile", do_set_from_file);
    super::define(it, "main::CleanFilename", clean_filename);
    super::define(it, "main::HasWildcards", has_wildcards);
    super::define(it, "main::CheckUTF8", check_utf8);
    super::define(it, "main::SetWindowTitle", set_window_title);
}
