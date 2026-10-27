//! Image::ExifTool::Trailer (Image/ExifTool/Trailer.pm), ported sub by sub (see docs/port/PORTING.md).
//!
//! Read JPEG trailer information

use crate::interp::eval::ER;
use crate::interp::{Interp, Want};
use crate::modules::random_access::cell;
use crate::modules::write_exif::{etf, get, len, put, read_eq, seek, tell};
use crate::perl::Value;
use crate::pl::{self, arg, cap, re, ret};

fn n(i: i64) -> Value {
    Value::Int(i)
}

fn s(x: &str) -> Value {
    Value::str(x)
}

/// `$s =~ /PAT/g` in scalar context from `pos`: on a match, the captures
/// and perl's new `pos`; a failed match resets `pos` to 0 (undef).
fn match_g(pat: &[u8], flags: &str, subject: &Value, pos: &mut usize) -> ER<Option<pl::Caps>> {
    let r = match crate::perl::regex::compile(pat, flags) {
        Ok(r) => r,
        Err(e) => return crate::interp::eval::die(e),
    };
    let b = subject.to_bytes();
    match r.match_at(&b, (*pos).min(b.len())) {
        Ok(Some(sp)) => {
            *pos = sp[0].map(|x| x.1).unwrap_or(*pos);
            Ok(Some(sp.iter().map(|g| g.map(|(a, e)| Value::bytes(&b[a..e]))).collect()))
        }
        _ => {
            *pos = 0;
            Ok(None)
        }
    }
}

/// `@$dirInfo{'DataPos','DirLen'} = ($pos, $len)`
fn set_pos_len(dir_info: &Value, pos: Value, l: Value) {
    dir_info.set("DataPos", pos);
    dir_info.set("DirLen", l);
}

/// Process Vivo trailer
/// Inputs: 0) ExifTool object reference, 1) dirInfo reference
/// Returns: 1 on success, 0 on failure, -1 if we must scan for the start
///          of the trailer to set the ExifTool TrailerStart member
/// - takes Offset as positive offset from end of trailer to end of file,
///   and returns DataPos and DirLen, and updates OutFile when writing
pub fn process_vivo(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info) = (arg(&a, 0), arg(&a, 1));
    let raf = dir_info.get("RAF");
    let buff = cell(Value::Undef);

    // return now unless we are at a position to scan for the trailer
    // (must scan because the trailer footer doesn't indicate the trailer length)
    if !dir_info.get("ScanForTrailer").truthy() {
        return ret(n(-1));
    }

    let mut pos = et.get("TrailerStart");
    if !pos.truthy() {
        return ret(n(0));
    }
    let mut l = pl::sub(&pl::sub(&et.get("FileEnd"), &pos), &dir_info.get("Offset"));
    if !seek(it, &raf, pos.clone(), 0)? {
        return ret(n(0));
    }
    let mut bpos: usize = 0; // pos($buff)
    let mut m1 = Value::Undef;
    let ok = pl::gt(&l, &n(0))
        && pl::lt(&l, &Value::Num(1e7))
        && read_eq(it, &raf, &buff, &l)?
        && re(r"\xff{4}\x1b\*9HWfu\x84\x93\xa2\xb1$", "").is_match(&get(&buff)) // validate footer
        && match match_g(br#"(streamdata|vivo\{")"#, "", &get(&buff), &mut bpos)? {
            // find start
            Some(c) => {
                m1 = cap(&c, 1);
                true
            }
            None => false,
        };
    if !ok {
        return ret(n(0));
    }
    let start = bpos as i64 - len(&m1).to_i64();
    if start != 0 {
        pos = pl::add(&pos, &n(start));
        l = pl::sub(&l, &n(start));
        put(&buff, pl::substr(&get(&buff), start, None));
    }
    // set trailer position and length
    set_pos_len(&dir_info, pos, l);

    // let ProcessTrailers copy or delete this trailer
    if dir_info.get("OutFile").truthy() {
        return ret(n(-1));
    }

    if et.get("OPTIONS").get("Verbose").truthy() || et.get("HTML_DUMP").truthy() {
        pl::method(it, &et, "DumpTrailer", vec![dir_info.clone()])?;
    }
    let tbl = etf(it, "GetTagTable", vec![s("Image::ExifTool::Trailer::Vivo")])?;
    bpos = 0; // rewind search to start of buffer
    if re(r"^streamdata\xff\xd8\xff", "").is_match(&get(&buff)) && match_g(br"\xff\xd9stream(info|coun)", "", &get(&buff), &mut bpos)?.is_some() {
        let v = pl::substr(&get(&buff), 10, Some(bpos as i64 - 20));
        pl::method(it, &et, "HandleTag", vec![tbl.clone(), s("HDRImage"), v])?;
    }
    // continue looking for Vivo JSON data
    if match_g(br#"vivo\{""#, "", &get(&buff), &mut bpos)?.is_some() {
        let json_start = bpos as i64 - 2;
        if match_g(br"\}\0", "", &get(&buff), &mut bpos)?.is_some() {
            let json_len = bpos as i64 - 1 - json_start;
            let v = pl::substr(&get(&buff), json_start, Some(json_len));
            pl::method(it, &et, "HandleTag", vec![tbl.clone(), s("JSONInfo"), v])?;
        }
    }
    ret(n(1))
}

/// Process OnePlus trailer
/// Inputs: 0) ExifTool object reference, 1) dirInfo reference
/// Returns: 1 on success, 0 on failure, -1 if we must scan for the start
///          of the trailer to set the ExifTool TrailerStart member
/// - takes Offset as positive offset from end of trailer to end of file,
///   and returns DataPos and DirLen, and updates OutFile when writing
pub fn process_one_plus(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info) = (arg(&a, 0), arg(&a, 1));
    let raf = dir_info.get("RAF");
    let (buff, buf2) = (cell(Value::Undef), cell(Value::Undef));

    // return now unless we are at a position to scan for the trailer
    // (must scan because the trailer footer doesn't indicate the entire trailer length)
    if !dir_info.get("ScanForTrailer").truthy() {
        return ret(n(-1));
    }

    // return -1 to let ProcessTrailers copy or delete the entire trailer
    if dir_info.get("OutFile").truthy() {
        return ret(n(-1));
    }

    let start = et.get("TrailerStart");
    if !start.truthy() {
        return ret(n(0));
    }
    if !(seek(it, &raf, pl::sub(&n(-8), &dir_info.get("Offset")), 2)? && read_eq(it, &raf, &buff, &n(8))?) {
        return ret(n(0));
    }
    let end = tell(it, &raf)?; // (same as FileEnd - Offset)

    let opt_v = et.get("OPTIONS").get("Verbose");
    let dump = if opt_v.truthy() { opt_v } else { et.get("HTML_DUMP") };
    let tag_table = etf(it, "GetTagTable", vec![s("Image::ExifTool::Trailer::OnePlus")])?;
    let mut trail_len = n(0);
    if re(r"^jxrs...\0$", "").is_match(&get(&buff)) {
        let jlen = pl::unpack1("x4V", &get(&buff))?;
        let mut max_off = n(0);
        let jm8 = pl::sub(&jlen, &n(8));
        if pl::lt(&jlen, &pl::sub(&end, &start))
            && pl::gt(&jlen, &n(8))
            && pl::method(it, &raf, "Seek", vec![pl::sub(&end, &jlen)])?.truthy()
            && read_eq(it, &raf, &buff, &jm8)?
        {
            let mut b = get(&buff);
            re(r"\0+$", "").subst(&mut b, false, |_| Vec::new()); // remove trailing null(s)
            put(&buff, b);
            crate::runtime::require(it, "Image::ExifTool::Import")?;
            let list = pl::call(it, "Image::ExifTool::Import::ReadJSONObject", vec![Value::Undef, Value::ScalarRef(buff.clone())])?;
            if list.is_array() {
                for e in list.items() {
                    let off = e.get("offset");
                    if off.truthy() && pl::gt(&off, &max_off) {
                        max_off = off;
                    }
                }
                trail_len = pl::add(&max_off, &jlen);
                if dump.truthy() && trail_len.truthy() {
                    let d = Value::new_hash();
                    d.set("RAF", raf.clone());
                    d.set("DirName", s("OnePlus"));
                    d.set("DataPos", pl::sub(&end, &trail_len));
                    d.set("DirLen", trail_len.clone());
                    pl::method(it, &et, "DumpTrailer", vec![d])?;
                }
                pl::method(it, &et, "HandleTag", vec![tag_table.clone(), s("JSONInfo"), get(&buff)])?;
                for e in list.items() {
                    let (off, name, l) = (e.get("offset"), e.get("name"), e.get("length"));
                    if !(off.truthy() && name.truthy() && l.truthy()) {
                        continue;
                    }
                    let p = pl::sub(&pl::sub(&end, &jlen), &off);
                    if pl::method(it, &raf, "Seek", vec![p.clone()])?.truthy() && read_eq(it, &raf, &buf2, &l)? {
                        pl::method(it, &et, "HandleTag", vec![tag_table.clone(), name, get(&buf2), s("DataPos"), p, s("DataPt"), Value::ScalarRef(buf2.clone())])?;
                    }
                }
            } else {
                pl::method(it, &et, "HandleTag", vec![tag_table.clone(), s("JSONInfo"), get(&buff)])?;
                pl::method(it, &et, "Warn", vec![s("Error parsing OnePlus JSON information")])?;
            }
        }
    }
    set_pos_len(&dir_info, pl::sub(&end, &trail_len), trail_len);

    ret(n(1))
}

/// Process Google trailer
/// Inputs: 0) ExifTool object reference, 1) dirInfo reference
/// Returns: 1 on success, 0 on failure, -1 if we must scan for the start
///          of the trailer to set the ExifTool TrailerStart member
/// - this trailer won't be identified when writing because XMP isn't extracted then
pub fn process_google(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info) = (arg(&a, 0), arg(&a, 1));
    let raf = dir_info.get("RAF");
    let info = et.get("VALUE");

    let (mut tag, mut mime, mut l, mut pad) = (info.get("DirectoryItemSemantic"), info.get("DirectoryItemMime"), info.get("DirectoryItemLength"), info.get("DirectoryItemPadding"));

    if !(tag.is_array() && mime.is_array()) {
        pad = Value::Undef;
        tag = info.get("ContainerDirectoryItemDataURI");
        mime = info.get("ContainerDirectoryItemMime");
        l = info.get("ContainerDirectoryItemLength");
        if !(mime.is_array() && tag.is_array()) {
            et.delete("ProcessGoogleTrailer");
            return ret(n(0));
        }
    }
    // we need to know TrailerStart to be able to read/write this trailer
    if !dir_info.get("ScanForTrailer").truthy() {
        return ret(n(-1));
    }

    et.delete("ProcessGoogleTrailer"); // reset flag to process the Google trailer

    if dir_info.get("OutFile").truthy() {
        return ret(n(-1)); // let caller handle the writing
    }

    // sometimes DirectoryItemLength is missing the Primary entry
    if !l.is_array() {
        l = Value::new_array(vec![l]);
    }
    while l.len() < mime.len() {
        l.unshift(n(0));
    }

    let start = et.get("TrailerStart");
    if !start.truthy() {
        return ret(n(0));
    }
    let end = et.get("FileEnd"); // (ignore Offset for now because some entries may run into other trailers)

    let opt_v = et.get("OPTIONS").get("Verbose");
    let dump = if opt_v.truthy() { opt_v } else { et.get("HTML_DUMP") };
    let tag_table = etf(it, "GetTagTable", vec![s("Image::ExifTool::Trailer::Google")])?;

    // (ignore first entry: "Primary" or "primary_image")
    let buff = cell(Value::Undef);
    let buff_pt = Value::ScalarRef(buff.clone());
    let mut pos = n(0);
    let mut i: i64 = 1;
    while !mime.at(i).is_undef() {
        let mut more = pl::sub(&pl::sub(&end, &start), &pos);
        if pl::lt(&more, &n(16)) {
            break;
        }
        let (li, ti, mi) = (l.at(i), tag.at(i), mime.at(i));
        if !(li.truthy() && !ti.is_undef()) {
            i += 1;
            continue;
        }
        if pl::gt(&li, &more) {
            break;
        }
        let sp = pl::add(&start, &pos);
        if !(pl::method(it, &raf, "Seek", vec![sp.clone()])?.truthy() && read_eq(it, &raf, &buff, &n(16))? && pl::method(it, &raf, "Seek", vec![sp.clone()])?.truthy()) {
            break;
        }
        let regex: &[u8] = if mi.eq_str("image/jpeg") {
            br"\xff\xd8\xff[\xdb\xe0\xe1]"
        } else if mi.eq_str("video/mp4") {
            br"\0\0\0.ftyp(mp42|isom)"
        } else {
            let m = Value::bytes([&b"Google trailer "[..], &ti.to_bytes(), b" ", &mi.to_bytes(), b" not handled"].concat());
            pl::method(it, &et, "Warn", vec![m])?;
            i += 1;
            continue;
        };
        let anchored = [&b"^"[..], regex].concat();
        if pl::re_dyn(&anchored, "s")?.is_match(&get(&buff)) {
            if !read_eq(it, &raf, &buff, &li)? {
                break;
            }
        } else {
            if pos.truthy() {
                break; // don't skip unknown information again
            }
            if !read_eq(it, &raf, &buff, &more)? {
                break;
            }
            let grouped = [&b"("[..], regex, b")"].concat();
            let mut bpos = 0usize;
            let Some(c) = match_g(&grouped, "s", &get(&buff), &mut bpos)? else { break };
            pos = pl::add(&pos, &n(bpos as i64 - len(&cap(&c, 1)).to_i64()));
            more = pl::sub(&pl::sub(&end, &start), &pos);
            let _ = more;
            if pl::gt(&li, &pl::sub(&pl::sub(&end, &start), &pos)) {
                break;
            }
            put(&buff, pl::substr(&get(&buff), pos.to_i64(), Some(li.to_i64())));
        }
        if !tag_table.get(ti.to_bytes()).truthy() {
            let mut name = ti.clone();
            re(r"([^A-Za-z])([a-z])", "").subst(&mut name, true, |c| [&cap(c, 1).to_bytes()[..], &pl::ucfirst(&cap(c, 2)).to_bytes()].concat()); // capitalize words
            let _ = name;
            let name = pl::call(it, "Image::ExifTool::MakeTagName", vec![ti.clone()])?;
            let (typ, grp) = if mi.eq_str("image/jpeg") { ("Image", "Preview") } else { ("Video", "Video") };
            let msg = Value::bytes([&b"[adding Google:"[..], &name.to_bytes(), b"]\n"].concat());
            pl::method(it, &et, "VPrint", vec![n(0), et.get("INDENT"), msg])?;
            let g = Value::new_hash();
            g.set("2", s(grp));
            let ti_hash = Value::new_hash();
            ti_hash.set("Name", pl::concat(&name, &s(typ)));
            ti_hash.set("Groups", g);
            pl::call(it, "Image::ExifTool::AddTagToTable", vec![tag_table.clone(), ti.clone(), ti_hash])?;
        }
        if dump.truthy() {
            let d = Value::new_hash();
            d.set("RAF", raf.clone());
            d.set("DirName", ti.clone());
            d.set("DataPos", pl::add(&start, &pos));
            d.set("DirLen", li.clone());
            pl::method(it, &et, "DumpTrailer", vec![d])?;
        }
        pl::method(it, &et, "HandleTag", vec![tag_table.clone(), ti.clone(), buff_pt.clone(), s("DataPos"), pl::add(&start, &pos), s("DataPt"), buff_pt.clone()])?;
        // (haven't seen non-zero padding, but I assume this is how it works
        let pd = if pad.truthy() && pad.at(i).truthy() { pad.at(i) } else { n(0) };
        pos = pl::add(&pos, &pl::add(&li, &pd));
        i += 1;
    }
    if !tag.at(i).is_undef() && !mime.at(i).is_undef() {
        let m = Value::bytes([&b"Error reading "[..], &tag.at(i).to_bytes(), b" ", &mime.at(i).to_bytes(), b" from trailer"].concat());
        pl::method(it, &et, "Warn", vec![m, n(1)])?;
    }
    if !pos.truthy() {
        return ret(n(0));
    }

    set_pos_len(&dir_info, start, pos);

    ret(n(1))
}

pub fn register(it: &mut Interp) {
    it.define_native("Image::ExifTool::Trailer::ProcessVivo", process_vivo);
    it.define_native("Image::ExifTool::Trailer::ProcessOnePlus", process_one_plus);
    it.define_native("Image::ExifTool::Trailer::ProcessGoogle", process_google);
}
