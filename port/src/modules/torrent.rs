//! Image::ExifTool::Torrent (Image/ExifTool/Torrent.pm), ported sub by sub (see docs/port/PORTING.md).
//!
//! Read information from BitTorrent file
//!
//! The buffer's pos() is kept on the Rust side while the ported subs run
//! (`pos` arguments of the `*_core` functions), and read from / written back
//! to the Perl scalar when a sub is called through its Perl name.

use crate::interp::eval::ER;
use crate::interp::{Interp, Want};
use crate::modules::random_access::{cell, perl_eval, perl_eval1};
use crate::perl::regex as pre;
use crate::perl::Value;
use crate::pl::{self, arg, cap, n, re};

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

/// `pos($$dataPt)`
fn get_pos(it: &mut Interp, data_pt: &Value) -> ER<Option<usize>> {
    let d = cell(data_pt.clone());
    let v = perl_eval1(it, "pos($$d)", "main", &[("d", &d)])?;
    Ok(if v.is_undef() {
        None
    } else {
        Some(v.to_i64() as usize)
    })
}

/// `pos($$dataPt) = POS` (undef resets it).
fn set_pos(it: &mut Interp, data_pt: &Value, p: Option<usize>) -> ER<()> {
    let d = cell(data_pt.clone());
    let v = cell(match p {
        Some(p) => n(p as i64),
        None => Value::Undef,
    });
    perl_eval(
        it,
        "pos($$d) = $p",
        "main",
        &[("d", &d), ("p", &v)],
        Want::Void,
    )?;
    Ok(())
}

/// A scalar `$$dataPt =~ /PAT/g` from pos() `p`: the captures on a match
/// (and `p` set to its end); on failure `p` becomes None, as perl resets pos().
fn match_g(pat: &str, flags: &str, subject: &[u8], p: &mut Option<usize>) -> Option<pl::Caps> {
    let rx =
        pre::compile(pat.as_bytes(), flags).unwrap_or_else(|e| panic!("bad pattern /{pat}/: {e}"));
    let start = p.unwrap_or(0);
    let r = if start > subject.len() {
        Ok(None)
    } else {
        rx.match_at(subject, start)
    };
    match r {
        Ok(Some(sp)) => {
            *p = Some(sp[0].unwrap().1);
            Some(
                sp.iter()
                    .map(|g| g.map(|(a, b)| Value::bytes(&subject[a..b])))
                    .collect(),
            )
        }
        _ => {
            *p = None;
            None
        }
    }
}

/// `$$dataPt = VALUE` (which resets pos(), as perl does).
fn assign(data_pt: &Value, v: Value, pos: &mut Option<usize>) -> ER<()> {
    pl::set_deref(data_pt, v)?;
    *pos = None;
    Ok(())
}

/// ReadMore with pos($$dataPt) in `pos`.
fn read_more_core(
    it: &mut Interp,
    raf: &Value,
    data_pt: &Value,
    pos: &mut Option<usize>,
) -> ER<Value> {
    let mut buf2 = Value::Undef;
    let nb = read(it, raf, &mut buf2, n(65536))?;
    if !pl::num_eq(&nb, &n(65536)) {
        raf.set("BencodeEOF", n(1));
    }
    if nb.truthy() {
        let rest = pl::substr(&data_pt.deref(), pos.unwrap_or(0) as i64, None);
        assign(data_pt, pl::concat(&rest, &buf2), pos)?;
    }
    Ok(nb)
}

/// Read 64kB more data into buffer
/// Inputs: 0) RAF ref, 1) buffer ref
/// Returns: number of bytes read
/// Notes: Sets BencodeEOF element of RAF on end of file
pub fn read_more(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (raf, data_pt) = (arg(&a, 0), arg(&a, 1));
    let mut pos = get_pos(it, &data_pt)?;
    let r = read_more_core(it, &raf, &data_pt, &mut pos)?;
    set_pos(it, &data_pt, pos)?;
    pl::ret(r)
}

/// ReadBencode with pos($$dataPt) in `pos`.
fn read_bencode_core(
    it: &mut Interp,
    et: &Value,
    raf: &Value,
    data_pt: &Value,
    pos: &mut Option<usize>,
) -> ER<Value> {
    // read more if necessary (keep a minimum of 64 bytes in the buffer)
    let Some(p) = *pos else {
        return Ok(Value::Undef);
    };
    let remaining = pl::length(&data_pt.deref()).to_i64() - p as i64;
    if remaining < 64 && !raf.get("BencodeEOF").truthy() {
        read_more_core(it, raf, data_pt, pos)?;
    }

    // read next token
    let Some(m) = match_g(r"(.)", "s", &data_pt.deref().to_bytes(), pos).map(|m| cap(&m, 1)) else {
        return Ok(Value::Undef);
    };
    let mut val = Value::Undef;
    let tok = m;
    if pl::str_eq(&tok, "i") {
        // integer
        let Some(c) = match_g(r"\G(-?\d+)e", "", &data_pt.deref().to_bytes(), pos) else {
            return Ok(val);
        };
        val = cap(&c, 1);
    } else if pl::str_eq(&tok, "d") {
        // dictionary
        val = Value::new_hash();
        loop {
            let mut k = read_bencode_core(it, et, raf, data_pt, pos)?;
            if k.is_undef() {
                break;
            }
            // the key must be a byte string
            if k.is_ref() {
                if pl::ref_of(&k) != "SCALAR" {
                    raf.set("BencodeError", pl::s("Bad dictionary key"));
                    break;
                }
                k = k.deref();
            }
            let v = read_bencode_core(it, et, raf, data_pt, pos)?;
            if v.is_undef() {
                break;
            }
            val.set(k.to_bytes(), v);
        }
    } else if pl::str_eq(&tok, "l") {
        // list
        val = Value::new_array(Vec::new());
        loop {
            let v = read_bencode_core(it, et, raf, data_pt, pos)?;
            if v.is_undef() {
                break;
            }
            val.push(v);
        }
    } else if pl::str_eq(&tok, "e") {
        // end of dictionary or list
        // return undef (no error)
    } else if let Some(c) = re(r"^\d$", "")
        .is_match(&tok)
        .then(|| match_g(r"\G(\d*):", "", &data_pt.deref().to_bytes(), pos))
        .flatten()
    {
        // byte string
        let len = pl::concat(&tok, &cap(&c, 1));
        let cur = pos.unwrap_or(0) as i64;
        let more = pl::sub(&len, &pl::sub(&pl::length(&data_pt.deref()), &n(cur)));
        let mut value = Value::Undef;
        if pl::le(&more, &n(0)) {
            value = pl::substr(&data_pt.deref(), cur, Some(len.to_i64()));
            *pos = Some((cur + len.to_i64()) as usize);
        } else if pl::gt(&more, &n(10000000)) {
            // just skip over really long values
            if pl::method(it, raf, "Seek", vec![more.clone(), n(1)])?.truthy() {
                val = pl::sref(pl::concat(
                    &pl::concat(&pl::s("(Binary data "), &len),
                    &pl::s(" bytes)"),
                ));
            }
        } else {
            // need to read more from file
            let mut buff = Value::Undef;
            let nb = read(it, raf, &mut buff, more.clone())?;
            if pl::num_eq(&nb, &more) {
                value = pl::concat(&pl::substr(&data_pt.deref(), cur, None), &buff);
                assign(data_pt, pl::s(""), pos)?;
                *pos = Some(0);
            }
        }
        if !value.is_undef() {
            // return as binary data unless it is a reasonable-length ASCII string
            if pl::length(&value).to_i64() > 256 {
                val = pl::sref(value);
            } else if re(r"[^\t\x20-\x7e]", "").is_match(&value) {
                if pl::ge(&call(it, "IsUTF8", vec![pl::sref(value.clone())])?, &n(0)) {
                    val = pl::method(it, et, "Decode", vec![value, pl::s("UTF8")])?;
                } else {
                    val = pl::sref(value);
                }
            } else {
                val = value;
            }
        } else if val.is_undef() {
            raf.set("BencodeError", pl::s("Truncated byte string"));
        }
    } else {
        raf.set("BencodeError", pl::s("Bad format"));
    }
    Ok(val)
}

/// Read bencoded value
/// Inputs: 0) ExifTool ref, 1) input file, 2) buffer (pos must be set to current position)
/// Returns: HASH ref, ARRAY ref, SCALAR ref, SCALAR, or undef on error or end of data
/// Notes: Sets BencodeError element of RAF on any error
pub fn read_bencode(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, raf, data_pt) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    let mut pos = get_pos(it, &data_pt)?;
    let r = read_bencode_core(it, &et, &raf, &data_pt, &mut pos)?;
    set_pos(it, &data_pt, pos)?;
    pl::ret(r)
}

/// Extract tags from dictionary hash
/// Inputs: 0) ExifTool ref, 1) dictionary hash reference, 2) tag table ref,
///         3) parent hash ID, 4) parent hash name, 5-N) list indices
/// Returns: number of tags extracted
pub fn extract_tags(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, hash_ptr, tag_table_ptr, base_id, base_name) =
        (arg(&a, 0), arg(&a, 1), arg(&a, 2), arg(&a, 3), arg(&a, 4));
    let mut index: Vec<Value> = a.iter().skip(5).cloned().collect();
    let mut count = n(0);
    let mut keys = hash_ptr.keys();
    keys.sort();
    for tag in keys {
        let tag = Value::bytes(&tag[..]);
        let mut val = hash_ptr.get(tag.to_bytes());
        let mut i: Option<Value> = None;
        let mut more: Vec<Value> = Vec::new();
        while !val.is_undef() {
            'body: {
                let mut id = if base_id.is_undef() {
                    tag.clone()
                } else {
                    pl::concat(&pl::concat(&base_id, &pl::s("/")), &tag)
                };
                if !tag_table_ptr.get(id.to_bytes()).truthy() {
                    let mut name = pl::ucfirst(&tag);
                    // capitalize all words in tag name and remove illegal characters
                    re(r"[^-_a-zA-Z0-9]+(.?)", "").subst(&mut name, true, |c| {
                        cap(c, 1).to_bytes().to_ascii_uppercase()
                    });
                    if pl::length(&name).to_i64() < 2 || !re(r"^[A-Z]", "").is_match(&name) {
                        name = pl::concat(&pl::s("Tag"), &name);
                    }
                    if !base_name.is_undef() {
                        name = pl::concat(&base_name, &name); // add base name if necessary
                    }
                    let h = Value::new_hash();
                    h.set("Name", name.clone());
                    call(
                        it,
                        "AddTagToTable",
                        vec![tag_table_ptr.clone(), id.clone(), h],
                    )?;
                    let msg = pl::concat(
                        &pl::concat(
                            &pl::concat(&pl::concat(&pl::s("  [adding "), &id), &pl::s(" '")),
                            &name,
                        ),
                        &pl::s("']\n"),
                    );
                    pl::method(it, &et, "VPrint", vec![n(0), msg])?;
                }
                let mut tag_info = pl::method(
                    it,
                    &et,
                    "GetTagInfo",
                    vec![tag_table_ptr.clone(), id.clone()],
                )?;
                if !tag_info.truthy() {
                    break 'body;
                }
                if pl::ref_of(&val) == "ARRAY" {
                    if tag_info.get("JoinPath").truthy() {
                        let parts: Vec<Value> = val
                            .items()
                            .into_iter()
                            .map(|x| {
                                if x.is_ref() {
                                    pl::s("(Binary data)")
                                } else {
                                    x
                                }
                            })
                            .collect();
                        val = pl::join("/", &parts);
                    } else {
                        if val.is_empty() {
                            break 'body; // ignore empty arrays
                        }
                        more.extend(val.items());
                        if pl::ref_of(&more[0]) == "ARRAY" {
                            break 'body; // continue expanding nested lists
                        }
                        val = more.remove(0);
                        if !i.as_ref().is_some_and(|x| x.truthy()) {
                            i = Some(n(0));
                            index.push(n(0));
                        }
                    }
                }
                if let Some(iv) = &i {
                    let ni = pl::add(iv, &n(1));
                    i = Some(ni.clone());
                    let last = index.len() - 1;
                    index[last] = ni;
                }
                if !index.is_empty() {
                    id = pl::concat(&id, &pl::join("_", &index)); // add instance number(s) to tag ID
                    if !tag_table_ptr.get(id.to_bytes()).truthy() {
                        let mut name = tag_info.get("Name");
                        // embed indices at position of '1' in tag name
                        let nn = pl::tr(&mut name, "1", "#", "").to_i64();
                        let mut j = 0usize;
                        while (j as i64) < nn {
                            let x = index.get(j).cloned().unwrap_or_default();
                            let idx = if x.truthy() { x } else { pl::s("") };
                            re("#", "").subst(&mut name, false, |_| idx.to_bytes().to_vec());
                            j += 1;
                        }
                        // put remaining indices at end of tag name
                        while j < index.len() {
                            if re(r"\d$", "").is_match(&name) {
                                name = pl::concat(&name, &pl::s("_"));
                            }
                            name = pl::concat(&name, &index[j]);
                            j += 1;
                        }
                        let h = tag_info.hash_copy();
                        h.set("Name", name);
                        call(
                            it,
                            "AddTagToTable",
                            vec![tag_table_ptr.clone(), id.clone(), h],
                        )?;
                    }
                    tag_info = pl::method(
                        it,
                        &et,
                        "GetTagInfo",
                        vec![tag_table_ptr.clone(), id.clone()],
                    )?;
                    if !tag_info.truthy() {
                        break 'body;
                    }
                }
                if pl::ref_of(&val) == "HASH" {
                    if pl::method(it, &et, "Options", vec![pl::s("Struct")])?.truthy()
                        && tag_info.truthy()
                        && pl::str_eq(&tag_info.get("Name"), "Info")
                    {
                        pl::method(it, &et, "FoundTag", vec![tag_info.clone(), val.clone()])?;
                        count = pl::add(&count, &n(1));
                        break 'body;
                    }
                    // extract tags from this dictionary
                    let (table, root_id, root_name);
                    if tag_info.get("SubDirectory").truthy() {
                        table = call(
                            it,
                            "GetTagTable",
                            vec![tag_info.get("SubDirectory").get("TagTable")],
                        )?;
                        root_id = Value::Undef;
                        root_name = Value::Undef;
                    } else {
                        table = tag_table_ptr.clone();
                        // use hash ID and Name as base for contained tags to avoid conflicts
                        root_id = id.clone();
                        root_name = tag_info.get("Name");
                    }
                    let mut args = vec![et.clone(), val.clone(), table, root_id, root_name];
                    args.extend(index.iter().cloned());
                    let c = extract_tags(it, args, Want::Scalar)?
                        .pop()
                        .unwrap_or_default();
                    count = pl::add(&count, &c);
                } else {
                    // handle this simple tag value
                    pl::method(
                        it,
                        &et,
                        "HandleTag",
                        vec![tag_table_ptr.clone(), id.clone(), val.clone()],
                    )?;
                    count = pl::add(&count, &n(1));
                }
            }
            val = if more.is_empty() {
                Value::Undef
            } else {
                more.remove(0)
            };
        }
        if i.is_some() {
            index.pop();
        }
    }
    pl::ret(count)
}

/// Process BitTorrent file
/// Inputs: 0) ExifTool object reference, 1) dirInfo reference (with RAF set)
/// Returns: 1 on success, 0 if this wasn't a valid BitTorrent file
pub fn process_torrent(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info) = (arg(&a, 0), arg(&a, 1));
    let mut success = n(0);
    let raf = dir_info.get("RAF");
    let buff = pl::sref(pl::s(""));
    let mut pos = Some(0);
    let dict = read_bencode_core(it, &et, &raf, &buff, &mut pos)?;
    let err = raf.get("BencodeError");
    if err.truthy() {
        pl::method(
            it,
            &et,
            "Warn",
            vec![pl::concat(&pl::s("Bencode error: "), &err)],
        )?;
    }
    if pl::ref_of(&dict) == "HASH"
        && (dict.get("announce").truthy()
            || dict.get("created by").truthy()
            || dict.get("info").truthy())
    {
        pl::method(it, &et, "SetFileType", vec![])?;
        let tag_table_ptr = call(
            it,
            "GetTagTable",
            vec![pl::s("Image::ExifTool::Torrent::Main")],
        )?;
        if extract_tags(it, vec![et.clone(), dict, tag_table_ptr], Want::Scalar)?
            .pop()
            .unwrap_or_default()
            .truthy()
        {
            success = n(1);
        }
    }
    pl::ret(success)
}

pub fn register(it: &mut Interp) {
    it.define_native("Image::ExifTool::Torrent::ReadMore", read_more);
    it.define_native("Image::ExifTool::Torrent::ReadBencode", read_bencode);
    it.define_native("Image::ExifTool::Torrent::ExtractTags", extract_tags);
    it.define_native("Image::ExifTool::Torrent::ProcessTorrent", process_torrent);
}
