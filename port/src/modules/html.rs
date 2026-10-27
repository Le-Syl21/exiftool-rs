//! Image::ExifTool::HTML (Image/ExifTool/HTML.pm), ported sub by sub (see docs/port/PORTING.md).
//!
//! Read HTML meta information

use crate::interp::eval::ER;
use crate::interp::{Interp, Want};
use crate::modules::random_access::perl_eval1;
use crate::perl::regex as pre;
use crate::perl::Value;
use crate::pl::{self, arg, cap, n, re};

const MODULE: &str = "Image::ExifTool::HTML";

/// `Image::ExifTool::NAME(ARGS)` (the functions imported with :DataAccess :Utils).
fn call(it: &mut Interp, name: &str, args: Vec<Value>) -> ER<Value> {
    pl::call(it, &format!("Image::ExifTool::{name}"), args)
}

/// `"a" . "b" . ...`
fn cat(parts: &[Value]) -> Value {
    let mut v = Vec::new();
    for p in parts {
        v.extend_from_slice(&p.to_bytes());
    }
    Value::bytes(v)
}

/// A `//g` match in scalar context from `start` (pos()): the captures and
/// the end of the match (the new pos()), or None (pos() is then reset).
fn match_g(rx: &pre::PerlRegex, subject: &[u8], start: usize) -> Option<(pl::Caps, usize)> {
    if start > subject.len() {
        return None;
    }
    match rx.match_at(subject, start) {
        Ok(Some(sp)) => {
            let end = sp[0].unwrap().1;
            Some((
                sp.iter()
                    .map(|g| g.map(|(a, b)| Value::bytes(&subject[a..b])))
                    .collect(),
                end,
            ))
        }
        _ => None,
    }
}

fn compile(pat: &[u8], flags: &str) -> ER<std::rc::Rc<pre::PerlRegex>> {
    match pre::compile(pat, flags) {
        Ok(r) => Ok(r),
        Err(e) => crate::interp::eval::die(e),
    }
}

/// Set HTMLCharset member based on content type
/// Inputs: 0) content type string, 1) ExifTool ref
/// Returns: original string
pub fn set_html_charset(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (val, et) = (arg(&a, 0), arg(&a, 1));
    if let Some(m) = re(r#"charset=['"]?([-\w]+)"#, "").m(&val) {
        let html_charset = crate::runtime::lex_hash(it, MODULE, "htmlCharset");
        et.set(
            "HTMLCharset",
            html_charset.get(pl::lc(&cap(&m, 1)).to_bytes()),
        );
    }
    pl::ret(val)
}

/// Convert single UTF-8 character to HTML character reference
/// Inputs: 0) UTF-8 character sequence
/// Returns: HTML character reference (eg. "&quot;");
/// Note: Must be called via EscapeHTML to load name lookup
pub fn escape_char(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let ch = arg(&a, 0);
    // (perl 5.10 or later: the meaning of "U0" is reversed as of Perl 5.10.0!)
    let val = pl::unpack("C0U", &ch)?
        .into_iter()
        .next()
        .unwrap_or_default();
    if val.is_undef() {
        return pl::ret(pl::s("?"));
    }
    let entity_name = crate::runtime::lex_hash(it, MODULE, "entityName");
    let name = entity_name.get(val.to_bytes());
    if name.truthy() {
        return pl::ret(cat(&[pl::s("&"), name, pl::s(";")]));
    }
    pl::ret(pl::sprintf("&#x%x;", &[val]))
}

/// `s/([\xc2-\xf7][\x80-\xbf]+)/EscapeChar($1)/sge`
fn escape_chars(it: &mut Interp, s: &mut Value) -> ER<()> {
    let rx = re(r"([\xc2-\xf7][\x80-\xbf]+)", "s");
    let mut err = None;
    rx.subst(s, true, |c| {
        if err.is_some() {
            return Vec::new();
        }
        match escape_char(it, vec![cap(c, 1)], Want::Scalar) {
            Ok(mut v) => v.pop().unwrap_or_default().to_bytes().to_vec(),
            Err(e) => {
                err = Some(e);
                Vec::new()
            }
        }
    });
    match err {
        Some(e) => Err(e),
        None => Ok(()),
    }
}

/// Escape any special characters for HTML
/// Inputs: 0) string to be escaped, 1) optional string encoding (default 'UTF8')
/// Returns: escaped string
pub fn escape_html(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (str_, enc) = (arg(&a, 0), arg(&a, 1));
    // escape XML characters
    let mut str_ = pl::call(it, "Image::ExifTool::XMP::EscapeXML", vec![str_])?;
    // escape other special characters if they exist
    if re(r"[\x80-\xff]", "").is_match(&str_) {
        // generate entity name lookup if necessary
        let entity_name = crate::runtime::lex_hash(it, MODULE, "entityName");
        if entity_name.keys().is_empty() {
            let entity_num = crate::runtime::lex_hash(it, MODULE, "entityNum");
            for k in entity_num.keys() {
                entity_name.set(entity_num.get(&k[..]).to_bytes(), Value::bytes(&k[..]));
            }
            entity_name.delete("39"); // 'apos' is not valid HTML
        }
        // suppress warnings
        let handler = perl_eval1(it, "sub { 1 }", MODULE, &[])?;
        str_ = pl::local_sig_warn(it, handler, |it| {
            if enc.truthy() && !pl::str_eq(&enc, "UTF8") {
                let mut s = call(
                    it,
                    "Decode",
                    vec![
                        Value::Undef,
                        str_.clone(),
                        enc.clone(),
                        Value::Undef,
                        pl::s("UTF8"),
                    ],
                )?;
                escape_chars(it, &mut s)?;
                call(
                    it,
                    "Decode",
                    vec![Value::Undef, s, pl::s("UTF8"), Value::Undef, enc.clone()],
                )
            } else {
                // escape any non-ascii characters for HTML
                let mut s = str_.clone();
                escape_chars(it, &mut s)?;
                Ok(s)
            }
        })?;
    }
    pl::ret(str_)
}

/// Unescape all HTML character references
/// Inputs: 0) string to be unescaped, 1) optional string encoding (default 'UTF8')
/// Returns: unescaped string
pub fn unescape_html(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (str_, enc) = (arg(&a, 0), arg(&a, 1));
    let entity_num = crate::runtime::lex_hash(it, MODULE, "entityNum");
    pl::ret(pl::call(
        it,
        "Image::ExifTool::XMP::UnescapeXML",
        vec![str_, entity_num, enc],
    )?)
}

/// `chr(hex($1))` in a tag name: a byte, or (for a wide character, which
/// the following `tr///dc` removes anyway) a byte that tr also removes.
fn chr_hex(h: &Value) -> Vec<u8> {
    let v = u32::from_str_radix(&h.to_text(), 16).unwrap_or(0);
    if v < 256 {
        vec![v as u8]
    } else {
        vec![0x80]
    }
}

/// Extract information from a HTML file
/// Inputs: 0) ExifTool object reference, 1) DirInfo reference
/// Returns: 1 on success, 0 if this wasn't a valid HTML file
pub fn process_html(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info) = (arg(&a, 0), arg(&a, 1));
    let raf = dir_info.get("RAF");
    let mut buff = Value::Undef;

    // validate HTML or XHTML file
    {
        let mut args = [buff.clone(), n(256)];
        let r = pl::method_mut(it, &raf, "Read", &mut args, Want::Scalar)?;
        buff = args[0].clone();
        if !r.into_iter().next().unwrap_or_default().truthy() {
            return pl::ret(n(0));
        }
    }
    let Some(m) = re(r"^(\xef\xbb\xbf)?\s*<(!DOCTYPE\s+HTML|HTML|\?xml)", "i").m(&buff) else {
        return pl::ret(n(0));
    };
    if pl::str_eq(&cap(&m, 2), "?xml") && !re(r"<(!DOCTYPE\s+)?HTML", "i").is_match(&buff) {
        return pl::ret(n(0));
    }
    pl::method(it, &et, "SetFileType", vec![])?;

    if !pl::method(it, &raf, "Seek", vec![n(0), n(0)])?.truthy() {
        pl::method(it, &et, "Warn", vec![pl::s("Seek error")])?;
        return pl::ret(n(1));
    }

    let sep = pl::call(
        it,
        "Image::ExifTool::PostScript::GetInputRecordSeparator",
        vec![raf.clone()],
    )?;
    pl::local_global(it, b'$', "main::/", |it| {
        it.set_scalar("main::/", sep.clone());
        if !sep.truthy() {
            pl::method(it, &et, "Warn", vec![pl::s("Invalid HTML data")])?;
            return pl::ret(n(1));
        }
        process_html_body(it, &et, &raf, &mut buff)
    })
}

/// ProcessHTML from the header extraction on (inside `local $/`).
fn process_html_body(it: &mut Interp, et: &Value, raf: &Value, buff: &mut Value) -> ER<Vec<Value>> {
    // extract header information
    let mut doc = Value::Undef;
    let head_rx = compile(br"<head\b", "i")?;
    loop {
        let mut args = [buff.clone()];
        let r = pl::method_mut(it, raf, "ReadLine", &mut args, Want::Scalar)?;
        *buff = args[0].clone();
        if !r.into_iter().next().unwrap_or_default().truthy() {
            break;
        }
        if doc.is_undef() {
            // look for 'head' element
            let b = buff.to_bytes();
            let Some((_, end)) = match_g(&head_rx, &b, 0) else {
                continue;
            };
            doc = pl::substr(buff, end as i64, None);
            continue;
        }
        doc = pl::concat(&doc, buff);
        if re(r"</head>", "i").is_match(buff) {
            break;
        }
    }
    if doc.is_undef() {
        return pl::ret(n(1));
    }

    // process all elements in header
    let tag_table_ptr = call(
        it,
        "GetTagTable",
        vec![pl::s("Image::ExifTool::HTML::Main")],
    )?;
    let doc_b = doc.to_bytes();
    let elem_rx = compile(br"<([\w:.-]+)(.*?)>", "s")?;
    let mut doc_pos: usize = 0; // pos($doc)
    loop {
        let Some((m, end)) = match_g(&elem_rx, &doc_b, doc_pos) else {
            break;
        };
        doc_pos = end;
        let (mut tag_name, attrs) = (cap(&m, 1), cap(&m, 2));
        let mut tag = pl::lc(&tag_name);
        let mut val;
        let mut grp = Value::Undef;
        if re(r"/$", "").is_match(&attrs) {
            // self-contained XHTML tags end in '/>'
            val = pl::s("");
        } else {
            // look for element close
            let pos = doc_pos;
            let close = cat(&[pl::s("</"), tag_name.clone(), pl::s(">")]);
            // the following doesn't work on Solaris Perl 5.6.1 due to Perl bug:
            // if ($doc =~ m{(.*?)</$tagName>}sg) {
            //     $val = $1;
            let close_rx = compile(&close.to_bytes(), "s")?;
            if let Some((_, end)) = match_g(&close_rx, &doc_b, doc_pos) {
                doc_pos = end;
                val = pl::substr(
                    &doc,
                    pos as i64,
                    Some(doc_pos as i64 - pos as i64 - pl::length(&close).to_i64()),
                );
            } else {
                doc_pos = pos;
                if !pl::str_eq(&tag, "meta") {
                    continue; // META tags don't need to be closed
                }
                val = pl::s("");
            }
        }
        let mut table = tag_table_ptr.clone();
        if pl::str_eq(&tag, "meta") {
            // parse HTML META element
            // tag name is in NAME or HTTP-EQUIV attribute
            if let Some(c) = re(r#"\bname\s*=\s*['"]?([\w:.-]+)"#, "si").m(&attrs) {
                tag_name = cap(&c, 1);
            } else if let Some(c) = re(r#"\bhttp-equiv\s*=\s*['"]?([\w:.-]+)"#, "si").m(&attrs) {
                tag_name = pl::concat(&pl::s("HTTP-equiv."), &cap(&c, 1));
            } else {
                continue; // no name
            }
            tag = pl::lc(&tag_name);
            if !tag.truthy() {
                continue;
            }
            // tag value is in CONTENT attribute
            if let Some(c) = re(r#"\bcontent\s*=\s*(['"])(.*?)\1"#, "si")
                .m(&attrs)
                .or_else(|| re(r#"\bcontent\s*=\s*(['"]?)([\w:.-]+)"#, "si").m(&attrs))
            {
                val = cap(&c, 2);
            } else if !pl::length(&val).truthy() {
                continue;
            }
            // isolate group name (separator is '.' in HTML, but ':' in ref 2)
            if let Some(c) = re(r"^([\w-]+)[:.]([\w-]+)", "").m(&tag) {
                grp = cap(&c, 1);
                tag = cap(&c, 2);
                let tag_info = pl::method(
                    it,
                    et,
                    "GetTagInfo",
                    vec![tag_table_ptr.clone(), grp.clone()],
                )?;
                if tag_info.truthy() && tag_info.get("SubDirectory").truthy() {
                    table = call(
                        it,
                        "GetTagTable",
                        vec![tag_info.get("SubDirectory").get("TagTable")],
                    )?;
                } else {
                    tag = cat(&[grp.clone(), pl::s("."), tag.clone()]);
                }
            }
        } else if pl::str_eq(&tag, "xml") {
            pl::method(it, et, "VPrint", vec![n(0), pl::s("Parsing XML\n")])?;
            // parse XML tags (quick-and-dirty)
            let xml = val.clone();
            for c in re(r"<([\w-]+):([\w-]+)(\s.*?)?>([^<]*?)<\/\1:\2>", "").each(&xml) {
                let (g, t, mut v) = (cap(&c, 1), cap(&c, 2), cap(&c, 4));
                let tag_info =
                    pl::method(it, et, "GetTagInfo", vec![tag_table_ptr.clone(), g.clone()])?;
                if !(tag_info.truthy() && tag_info.get("SubDirectory").truthy()) {
                    continue;
                }
                table = call(
                    it,
                    "GetTagTable",
                    vec![tag_info.get("SubDirectory").get("TagTable")],
                )?;
                if !table.get(t.to_bytes()).truthy() {
                    let mut name = pl::ucfirst(&t);
                    re(r"_x([0-9a-f]{4})_", "i").subst(&mut name, true, |c| chr_hex(&cap(c, 1))); // convert hex codes
                    re(r"\s(.)", "").subst(&mut name, true, |c| {
                        cap(c, 1).to_bytes().to_ascii_uppercase()
                    }); // capitalize all words in tag name
                    pl::tr(&mut name, "-_a-zA-Z0-9", "", "dc"); // remove illegal characters (also hex code wide chars)
                    let h = Value::new_hash();
                    h.set("Name", name.clone());
                    call(it, "AddTagToTable", vec![table.clone(), t.clone(), h])?;
                    pl::method(
                        it,
                        et,
                        "VPrint",
                        vec![
                            n(0),
                            cat(&[
                                pl::s("  [adding "),
                                t.clone(),
                                pl::s(" '"),
                                name,
                                pl::s("']\n"),
                            ]),
                        ],
                    )?;
                }
                if et.get("HTMLCharset").truthy() {
                    v = pl::method(it, et, "Decode", vec![v, et.get("HTMLCharset")])?;
                }
                let uv = pl::call(it, "Image::ExifTool::XMP::UnescapeXML", vec![v])?;
                pl::method(it, et, "HandleTag", vec![table.clone(), t, uv])?;
            }
            continue;
        } else {
            // the only other element we process is TITLE
            if !pl::str_eq(&tag, "title") {
                continue;
            }
        }
        if !table.get(tag.to_bytes()).truthy() {
            let mut name = tag_name.clone();
            re(r"\W+(\w)", "s").subst(&mut name, true, |c| {
                pl::ucfirst(&cap(c, 1)).to_bytes().to_vec()
            });
            let info = Value::new_hash();
            info.set("Name", name);
            let groups = Value::new_hash();
            groups.set("0", pl::s("HTML"));
            info.set("Groups", groups.clone());
            if grp.truthy() {
                groups.set(
                    "1",
                    if pl::str_eq(&grp, "http-equiv") {
                        pl::s("HTTP-equiv")
                    } else {
                        pl::concat(&pl::s("HTML-"), &grp)
                    },
                );
            }
            call(it, "AddTagToTable", vec![table.clone(), tag.clone(), info])?;
            pl::method(
                it,
                et,
                "VPrint",
                vec![
                    n(0),
                    cat(&[
                        pl::s("  [adding "),
                        tag.clone(),
                        pl::s(" '"),
                        tag_name.clone(),
                        pl::s("']\n"),
                    ]),
                ],
            )?;
        }
        // recode if necessary
        if et.get("HTMLCharset").truthy() {
            val = pl::method(it, et, "Decode", vec![val, et.get("HTMLCharset")])?;
        }
        let sep = it.get_scalar("main::/");
        let mut p = br"\s*".to_vec();
        p.extend_from_slice(&sep.to_bytes());
        p.extend_from_slice(br"\s*");
        pl::re_dyn(&p, "s")?.subst(&mut val, true, |_| b" ".to_vec()); // replace linefeeds and indenting spaces
        val = unescape_html(it, vec![val], Want::Scalar)?
            .pop()
            .unwrap_or_default(); // unescape HTML character references
        pl::method(it, et, "HandleTag", vec![table, tag, val])?;
    }
    pl::ret(n(1))
}

pub fn register(it: &mut Interp) {
    it.define_native("Image::ExifTool::HTML::SetHTMLCharset", set_html_charset);
    it.define_native("Image::ExifTool::HTML::EscapeChar", escape_char);
    it.define_native("Image::ExifTool::HTML::EscapeHTML", escape_html);
    it.define_native("Image::ExifTool::HTML::UnescapeHTML", unescape_html);
    it.define_native("Image::ExifTool::HTML::ProcessHTML", process_html);
}
