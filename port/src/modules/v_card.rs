//! Image::ExifTool::VCard (Image/ExifTool/VCard.pm), ported sub by sub (see docs/port/PORTING.md).
//!
//! Read vCard and iCalendar meta information

use crate::interp::eval::ER;
use crate::interp::{Interp, Want};
use crate::perl::Value;
use crate::pl::{self, arg, cap, n, re};

const MODULE: &str = "Image::ExifTool::VCard";

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

/// `$raf->ReadLine($buff)`: the result, with `buff` updated.
fn read_line(it: &mut Interp, raf: &Value, buff: &mut Value) -> ER<Value> {
    let mut args = [buff.clone()];
    let r = pl::method_mut(it, raf, "ReadLine", &mut args, Want::Scalar)?;
    *buff = args[0].clone();
    Ok(r.into_iter().next().unwrap_or_default())
}

/// `chomp $buff` (with the current `$/`)
fn chomp(it: &mut Interp, buff: &mut Value) {
    let sep = it.get_scalar("main::/");
    if sep.is_undef() || buff.is_undef() {
        return;
    }
    let (b, s) = (buff.to_bytes(), sep.to_bytes());
    if !s.is_empty() && b.ends_with(&s) {
        *buff = Value::bytes(&b[..b.len() - s.len()]);
    }
}

/// `s/\\(.)/$unescapeVCard{$1}||$1/sge`
fn unescape(it: &mut Interp, val: &mut Value) {
    let unescape_vcard = crate::runtime::lex_hash(it, MODULE, "unescapeVCard");
    re(r"\\(.)", "s").subst(val, true, |c| {
        let u = unescape_vcard.get(cap(c, 1).to_bytes());
        if u.truthy() {
            u.to_bytes().to_vec()
        } else {
            cap(c, 1).to_bytes().to_vec()
        }
    });
}

/// Get vCard tag, creating if necessary
/// Inputs: 0) ExifTool ref, 1) tag table ref, 2) tag ID, 3) tag Name,
///         4) source tagInfo ref, 5) lang code
/// Returns: tagInfo ref
pub fn get_v_card_tag(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, tag_table_ptr, tag, name, src_info, lang_code) = (
        arg(&a, 0),
        arg(&a, 1),
        arg(&a, 2),
        arg(&a, 3),
        arg(&a, 4),
        arg(&a, 5),
    );
    let mut tag_info = tag_table_ptr.get(tag.to_bytes());
    if !tag_info.truthy() {
        if src_info.truthy() {
            tag_info = src_info.hash_copy();
        } else {
            tag_info = Value::new_hash();
            pl::method(
                it,
                &et,
                "VPrint",
                vec![
                    n(0),
                    et.get("INDENT"),
                    cat(&[pl::s("[adding "), tag.clone(), pl::s("]\n")]),
                ],
            )?;
        }
        tag_info.set("Name", name);
        tag_info.delete("Description"); // create new description
        call(
            it,
            "AddTagToTable",
            vec![tag_table_ptr, tag, tag_info.clone()],
        )?;
    }
    // handle alternate languages (the "language" parameter)
    if lang_code.truthy() {
        tag_info = call(it, "GetLangInfo", vec![tag_info, lang_code])?;
    }
    pl::ret(tag_info)
}

fn get_v_card_tag1(it: &mut Interp, args: Vec<Value>) -> ER<Value> {
    Ok(get_v_card_tag(it, args, Want::Scalar)?
        .pop()
        .unwrap_or_default())
}

/// Decode vCard text
/// Inputs: 0) ExifTool ref, 1) vCard text, 2) encoding
/// Returns: decoded text (or array ref for a list of values)
pub fn decode_v_card_text(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, mut val, enc) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    let enc = if enc.is_undef() {
        pl::s("")
    } else {
        pl::lc(&enc)
    };
    if pl::str_eq(&enc, "b") || pl::str_eq(&enc, "base64") {
        crate::runtime::require(it, "Image::ExifTool::XMP")?;
        val = pl::call(it, "Image::ExifTool::XMP::DecodeBase64", vec![val])?;
    } else {
        if pl::str_eq(&enc, "quoted-printable") {
            // convert "=HH" hex codes to characters
            re(r"=([0-9a-f]{2})", "i").subst(&mut val, true, |c| {
                let h = u8::from_str_radix(&cap(c, 1).to_text(), 16).unwrap_or(0);
                vec![h]
            });
        }
        val = pl::method(it, &et, "Decode", vec![val, pl::s("UTF8")])?; // convert from UTF-8
                                                                        // convert unescaped commas to nulls to separate list items
        re(r"(\\.)|(,)", "s").subst(&mut val, true, |c| {
            let e = cap(c, 1);
            if e.truthy() {
                e.to_bytes().to_vec()
            } else {
                b"\0".to_vec()
            }
        });
        // unescape necessary characters in value
        unescape(it, &mut val);
        // split into list if necessary
        let vals = pl::split(&re(r"\0", ""), &val, 0);
        if vals.len() > 1 {
            val = Value::new_array(vals);
        }
    }
    pl::ret(val)
}

fn decode_v_card_text1(it: &mut Interp, args: Vec<Value>) -> ER<Value> {
    Ok(decode_v_card_text(it, args, Want::Scalar)?
        .pop()
        .unwrap_or_default())
}

/// Read information in a vCard file
/// Inputs: 0) ExifTool ref, 1) dirInfo ref
/// Returns: 1 on success, 0 if this wasn't a valid vCard file
pub fn process_v_card(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    pl::local_topic(it, |it| process_v_card_body(it, a))
}

fn process_v_card_body(it: &mut Interp, a: Vec<Value>) -> ER<Vec<Value>> {
    let (et, dir_info) = (arg(&a, 0), arg(&a, 1));
    let raf = dir_info.get("RAF");
    let mut buff = Value::Undef;

    let mut m = None;
    if read_line_n(it, &raf, &mut buff, 24)?.truthy()
        && pl::method(it, &raf, "Seek", vec![n(0), n(0)])?.truthy()
    {
        m = re(r"^BEGIN:(VCARD|VCALENDAR|VNOTE)\r\n", "i").m(&buff);
    }
    let Some(m) = m else {
        return pl::ret(n(0));
    };
    let info: &[&str] = match &*pl::uc(&cap(&m, 1)).to_bytes() {
        b"VCARD" => &["VCard", "vCard", "Main", "VCF"],
        b"VCALENDAR" => &["ICS", "iCalendar", "VCalendar", "ICS"],
        _ => &["VNote", "vNote", "VNote", "VNT", "text/v-note"],
    };
    let f = |i: usize| info.get(i).map_or(Value::Undef, |s| pl::s(s));
    let (typ, lbl, tbl, ext, mime) = (f(0), f(1), f(2), f(3), f(4));
    pl::method(it, &et, "SetFileType", vec![typ, mime, ext])?;
    let fast_scan = et.get("OPTIONS").get("FastScan");
    if fast_scan.truthy() && pl::num_eq(&fast_scan, &n(3)) {
        return pl::ret(n(1));
    }
    pl::local_global(it, b'$', "main::/", |it| {
        it.set_scalar("main::/", pl::s("\r\n"));
        process_lines(it, &et, &raf, &lbl, &tbl, &mut buff)
    })
}

/// `$raf->Read($buff, $len)`
fn read_line_n(it: &mut Interp, raf: &Value, buff: &mut Value, len: i64) -> ER<Value> {
    let mut args = [buff.clone(), n(len)];
    let r = pl::method_mut(it, raf, "Read", &mut args, Want::Scalar)?;
    *buff = args[0].clone();
    Ok(r.into_iter().next().unwrap_or_default())
}

/// The line loop of ProcessVCard (inside `local $/ = "\r\n"`).
fn process_lines(
    it: &mut Interp,
    et: &Value,
    raf: &Value,
    lbl: &Value,
    tbl: &Value,
    buff: &mut Value,
) -> ER<Vec<Value>> {
    let mut val = Value::Undef;
    let mut ok = Value::Undef;
    let mut component = Value::Undef;
    let comp_num = Value::new_hash();
    let mut count: Vec<Value> = Vec::new();
    let is_component = crate::runtime::lex_hash(it, MODULE, "isComponent");
    let tag_table_ptr = call(
        it,
        "GetTagTable",
        vec![pl::concat(&pl::s("Image::ExifTool::VCard::"), tbl)],
    )?;
    let mut more = read_line(it, raf, buff)?; // read first line
    if more.truthy() {
        chomp(it, buff);
    }
    while more.truthy() {
        // retrieve previous line from $buff
        if !buff.is_undef() {
            val = buff.clone();
        }
        // read ahead to next line to see if is a continuation
        more = read_line(it, raf, buff)?;
        if more.truthy() {
            chomp(it, buff);
            // add continuation line if necessary
            if re(r"^[ \t]", "").subst(buff, false, |_| Vec::new()) > 0 {
                val = pl::concat(&val, buff);
                *buff = Value::Undef;
                continue;
            }
        }
        if let Some(m) = re(r"^(BEGIN|END):(V?)(\w+)$", "i").m(&val) {
            let begin = pl::str_eq(&pl::lc(&cap(&m, 1)), "begin");
            let v = cap(&m, 2);
            let what = pl::ucfirst(&pl::lc(&cap(&m, 3)));
            if pl::str_eq(&what, "Card")
                || pl::str_eq(&what, "Calendar")
                || pl::str_eq(&what, "Note")
            {
                if begin {
                    count = vec![Value::new_hash()]; // reset group counters
                } else {
                    ok = n(1); // ok if we read at least on full VCARD or VCALENDAR
                }
                continue;
            }
            // absorb top-level component into family 1 group name
            if is_component.get(what.to_bytes()).truthy() {
                if begin {
                    if !component.truthy() {
                        // begin a new top-level component
                        count = vec![Value::new_hash()];
                        component = what.clone();
                        let c = comp_num.get(component.to_bytes());
                        comp_num.set(
                            component.to_bytes(),
                            pl::add(&if c.truthy() { c } else { n(0) }, &n(1)),
                        );
                        continue;
                    }
                } else if component.truthy() && component.to_bytes() == what.to_bytes() {
                    // this top-level component has ended
                    component = Value::Undef;
                    continue;
                }
            }
            // keep count of each component at this level
            if begin {
                if v.truthy() {
                    let Some(last) = count.last().cloned() else {
                        return crate::interp::eval::die(
                            "Modification of non-creatable array value attempted, subscript -1",
                        );
                    };
                    let c = last.get(what.to_bytes());
                    last.set(
                        what.to_bytes(),
                        pl::add(&if c.truthy() { c } else { n(0) }, &n(1)),
                    );
                }
                let h = Value::new_hash();
                h.set("obj", what);
                count.push(h);
            } else if count.len() > 1 {
                count.pop();
            }
            continue;
        } else if ok.truthy() {
            ok = n(0);
            let dc = pl::add(&et.get("DOC_COUNT"), &n(1));
            et.set("DOC_COUNT", dc.clone());
            et.set("DOC_NUM", dc); // read next card as a new document
        }
        let mut tag = Value::Undef;
        if re(r"^([-A-Za-z0-9.]+)", "").subst(&mut val, false, |c| {
            tag = cap(c, 1);
            Vec::new()
        }) == 0
        {
            pl::method(
                it,
                et,
                "Warn",
                vec![cat(&[
                    pl::s("Unrecognized line in "),
                    lbl.clone(),
                    pl::s(" file"),
                ])],
            )?;
            continue;
        }
        // set group if it exists
        let mut grp = Value::Undef;
        if re(r"^([-A-Za-z0-9]+)\.", "").subst(&mut tag, false, |c| {
            grp = cap(c, 1);
            Vec::new()
        }) > 0
        {
            et.set("SET_GROUP1", pl::ucfirst(&pl::lc(&grp)));
        } else if component.truthy() {
            et.set(
                "SET_GROUP1",
                pl::concat(&component, &comp_num.get(component.to_bytes())),
            );
        } else {
            et.delete("SET_GROUP1");
        }
        let mut name = Value::Undef;
        let param = Value::new_hash();
        // vCard tag ID's are case-insensitive, so normalize to lowercase with
        // an uppercase first letter for use as a tag name
        if re(r"[a-z]", "").is_match(&tag) {
            name = pl::ucfirst(&tag); // preserve mixed case in name if it exists
        }
        tag = pl::ucfirst(&pl::lc(&tag));
        // get source tagInfo reference
        let src_info = pl::method(
            it,
            et,
            "GetTagInfo",
            vec![tag_table_ptr.clone(), tag.clone()],
        )?;
        if src_info.truthy() {
            name = src_info.get("Name"); // use our name
        } else {
            if !name.truthy() {
                name = tag.clone();
            }
            // remove leading "X-" from name if it exists
            if re(r"^X-", "").subst(&mut name, false, |_| Vec::new()) > 0 {
                name = pl::ucfirst(&name);
            }
        }
        // add object name(s) to tag if necessary
        if count.len() > 1 {
            let mut i = count.len() as i64 - 2;
            while i >= 0 {
                let prev = if i >= 1 {
                    count[(i - 1) as usize].clone()
                } else {
                    count[count.len() - 1].clone()
                };
                let pre = prev.get("obj"); // use containing object name as tag prefix
                let mut c = count[i as usize].get(pre.to_bytes()); // add index for object number
                if c.is_undef() {
                    c = pl::s("");
                }
                tag = cat(&[pre.clone(), c.clone(), tag.clone()]);
                name = cat(&[pre, c, name.clone()]);
                i -= 1;
            }
        }
        // parse parameters
        loop {
            let mut c1 = Value::Undef;
            let mut c2 = Value::Undef;
            if re(r"^;([-A-Za-z0-9]*)(=?)", "").subst(&mut val, false, |c| {
                c1 = cap(c, 1);
                c2 = cap(c, 2);
                Vec::new()
            }) == 0
            {
                break;
            }
            let mut p = pl::ucfirst(&pl::lc(&c1));
            // convert old vCard 2.x parameters to the new "TYPE=" format
            if !c2.truthy() {
                val = pl::concat(&c1, &val);
                p = pl::s("Type");
            }
            // read parameter value
            loop {
                let mut v1 = Value::Undef;
                let got = re(r#"^"([^"]*)",?"#, "").subst(&mut val, false, |c| {
                    v1 = cap(c, 1);
                    Vec::new()
                }) > 0
                    || re(r#"^([^";:,]+,?)"#, "").subst(&mut val, false, |c| {
                        v1 = cap(c, 1);
                        Vec::new()
                    }) > 0;
                if !got {
                    break;
                }
                let v = if pl::str_eq(&p, "Type") {
                    pl::ucfirst(&pl::lc(&v1))
                } else {
                    v1
                };
                let old = param.get(p.to_bytes());
                param.set(
                    p.to_bytes(),
                    if !old.is_undef() {
                        pl::concat(&old, &v)
                    } else {
                        v
                    },
                );
            }
            if !param.get(p.to_bytes()).is_undef() {
                let mut pv = param.get(p.to_bytes());
                unescape(it, &mut pv);
                param.set(p.to_bytes(), pv);
            } else {
                param.set(p.to_bytes(), pl::s(""));
            }
        }
        if re(r"^:", "").subst(&mut val, false, |_| Vec::new()) == 0 {
            pl::method(
                it,
                et,
                "Warn",
                vec![cat(&[
                    pl::s("Invalid line in "),
                    lbl.clone(),
                    pl::s(" file"),
                ])],
            )?;
            continue;
        }
        // add 'Type' parameter to id and name if it exists
        if param.get("Type").truthy() {
            tag = pl::concat(&tag, &param.get("Type"));
            name = pl::concat(&name, &param.get("Type"));
        }
        // convert base64-encoded data
        let mut d = (Value::Undef, Value::Undef);
        if re(r"^data:(\w+)/(\w+);base64,", "").subst(&mut val, false, |c| {
            d = (cap(c, 1), cap(c, 2));
            Vec::new()
        }) > 0
        {
            let xtra = pl::concat(&pl::ucfirst(&pl::lc(&d.0)), &pl::ucfirst(&pl::lc(&d.1)));
            tag = pl::concat(&tag, &xtra);
            name = pl::concat(&name, &xtra);
            param.set("Encoding", pl::s("base64"));
        }
        val = decode_v_card_text1(it, vec![et.clone(), val.clone(), param.get("Encoding")])?;
        let mut tag_info = get_v_card_tag1(
            it,
            vec![
                et.clone(),
                tag_table_ptr.clone(),
                tag.clone(),
                name.clone(),
                src_info.clone(),
                param.get("Language"),
            ],
        )?;
        pl::method(
            it,
            et,
            "HandleTag",
            vec![
                tag_table_ptr.clone(),
                tag.clone(),
                val.clone(),
                pl::s("TagInfo"),
                tag_info.clone(),
            ],
        )?;
        // handle some other parameters that we care about (ignore the rest for now)
        for p in ["Geo", "Label", "Tzid"] {
            if param.get(p).is_undef() {
                continue;
            }
            // use tag attributes from our table if it exists
            let src_tag2 = pl::method(it, et, "GetTagInfo", vec![tag_table_ptr.clone(), pl::s(p)])?;
            let pn = if src_tag2.truthy() {
                src_tag2.get("Name")
            } else {
                pl::s(p)
            };
            val = decode_v_card_text1(it, vec![et.clone(), param.get(p)])?;
            // add parameter to tag ID and name
            let (tg, nm) = (pl::concat(&tag, &pl::s(p)), pl::concat(&name, &pn));
            tag_info = get_v_card_tag1(
                it,
                vec![
                    et.clone(),
                    tag_table_ptr.clone(),
                    tg.clone(),
                    nm,
                    src_tag2,
                    param.get("Language"),
                ],
            )?;
            pl::method(
                it,
                et,
                "HandleTag",
                vec![
                    tag_table_ptr.clone(),
                    tg,
                    val.clone(),
                    pl::s("TagInfo"),
                    tag_info.clone(),
                ],
            )?;
        }
    }
    et.delete("SET_GROUP1");
    et.delete("DOC_NUM");
    if !ok.truthy() {
        pl::method(
            it,
            et,
            "Warn",
            vec![cat(&[pl::s("Missing "), lbl.clone(), pl::s(" end")])],
        )?;
    }
    pl::ret(n(1))
}

pub fn register(it: &mut Interp) {
    it.define_native("Image::ExifTool::VCard::GetVCardTag", get_v_card_tag);
    it.define_native(
        "Image::ExifTool::VCard::DecodeVCardText",
        decode_v_card_text,
    );
    it.define_native("Image::ExifTool::VCard::ProcessVCard", process_v_card);
}
