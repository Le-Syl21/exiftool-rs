//! Image::ExifTool::Sony (Image/ExifTool/Sony.pm), ported sub by sub (see docs/port/PORTING.md).

use crate::interp::eval::{die, Cell, ER};
use crate::interp::{Interp, Want};
use crate::modules::exiftool::part2;
use crate::modules::olympus::hex;
use crate::modules::random_access::{cell, raf_read_cell, substr};
use crate::perl::{ops, Value};
use crate::pl::{self, arg, cap, n, re, ret};

const MODULE: &str = "Image::ExifTool::Sony";

fn val(c: &Cell) -> Value {
    c.borrow().clone()
}

/// `\$var` of a `my` variable kept in a cell.
fn rf(c: &Cell) -> Value {
    Value::ScalarRef(c.clone())
}

fn first(v: ER<Vec<Value>>) -> ER<Value> {
    Ok(v?.pop().unwrap_or_default())
}

fn g16u(it: &mut Interp, data_pt: &Value, pos: Value) -> ER<Value> {
    first(part2::get16u(it, vec![data_pt.clone(), pos], Want::Scalar))
}

fn g32u(it: &mut Interp, data_pt: &Value, pos: Value) -> ER<Value> {
    first(part2::get32u(it, vec![data_pt.clone(), pos], Want::Scalar))
}

/// `Set32u($val, $dataPt, $pos)`: writes into the data.
fn s32u_at(it: &mut Interp, v: Value, data_pt: &Value, pos: Value) -> ER<Value> {
    first(part2::set32u(it, vec![v, data_pt.clone(), pos], Want::Scalar))
}

/// `Set32u($val)`: the packed value.
fn s32u(it: &mut Interp, v: Value) -> ER<Value> {
    first(part2::set32u(it, vec![v], Want::Scalar))
}

fn get_tag_table(it: &mut Interp, name: &str) -> ER<Value> {
    pl::call(it, "Image::ExifTool::GetTagTable", vec![pl::s(name)])
}

/// `$a <=> $b` (undef for NaN).
fn spaceship(a: &Value, b: &Value) -> Value {
    match ops::num_cmp(a, b) {
        Some(std::cmp::Ordering::Less) => n(-1),
        Some(std::cmp::Ordering::Equal) => n(0),
        Some(std::cmp::Ordering::Greater) => n(1),
        None => Value::Undef,
    }
}

/// `substr($s, $off, $len) = $repl` (fatal outside the string, as perl).
fn substr_assign(s: &Value, off: &Value, len: &Value, repl: &Value) -> ER<Value> {
    let b = s.to_bytes();
    match crate::interp::eval::substr_range(b.len(), off.to_i64(), Some(len.to_i64())) {
        Some((a, e)) => {
            let mut v = b[..a].to_vec();
            v.extend_from_slice(&repl.to_bytes());
            v.extend_from_slice(&b[e..]);
            Ok(Value::bytes(v))
        }
        None => die("substr outside of string"),
    }
}

/// `$$ref` under strict refs.
fn deref_strict(v: &Value) -> ER<Value> {
    match v {
        Value::ScalarRef(_) | Value::LvRef(_) => Ok(v.deref()),
        Value::Undef => die("Can't use an undefined value as a SCALAR reference"),
        _ => die("Not a SCALAR reference"),
    }
}

/// `$$outfile .= $data`
fn append(out: &Value, data: &Value) -> ER<()> {
    let cur = out.deref();
    pl::set_deref(out, pl::concat(&cur, data))
}

/// `$x || $default`
fn or(v: Value, d: Value) -> Value {
    if v.truthy() {
        v
    } else {
        d
    }
}

//------------------------------------------------------------------------------
/// Sort lens types numerically, with the decimal part compared separately
/// (a `sort` comparator: reads `$a` and `$b`)
pub fn sort_lens_types(it: &mut Interp, _a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let a = it.get_scalar("Image::ExifTool::Sony::a");
    let b = it.get_scalar("Image::ExifTool::Sony::b");
    let dot = re(r"\.", "");
    if !(dot.is_match(&a) && dot.is_match(&b)) {
        return ret(spaceship(&a, &b));
    }
    let av = pl::split(&dot, &a, 0);
    let bv = pl::split(&dot, &b, 0);
    // must compare the decimal part separately to sort in proper order
    let c = spaceship(&arg(&av, 0), &arg(&bv, 0));
    if c.truthy() {
        return ret(c);
    }
    ret(spaceship(&arg(&av, 1), &arg(&bv, 1)))
}

//------------------------------------------------------------------------------
/// Read HiddenData from JPEG trailer
/// Inputs: 0) ExifTool ref, 1) HiddenDataOffset (abs), 2) HiddenDataLength
/// Returns: HiddenData reference, or undef on error
/// --> updates $hdOff upon return if it was incorrect
pub fn read_hidden_data(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, hd_off, hd_len) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    let raf = et.get("RAF");
    let buff = cell(Value::Undef);
    let mut pos = Value::Undef;
    let ok = pl::method(it, &raf, "Seek", vec![hd_off, n(0)])?.truthy()
        && pl::num_eq(&raf_read_cell(it, &raf, &buff, hd_len.clone())?, &hd_len)
        && re(r"^\x55\x26\x11\x05\0", "").is_match(&val(&buff));
    if !ok {
        // search the first 4096 bytes of the trailer to find the HiddenData
        let trailer_start = et.get("TrailerStart");
        let mut found = trailer_start.truthy()
            && pl::method(it, &raf, "Seek", vec![trailer_start.clone(), n(0)])?.truthy()
            && raf_read_cell(it, &raf, &buff, n(4096))?.truthy();
        if found {
            // $buff =~ /\x55\x26\x11\x05\0/g (pos($buff) starts at 0)
            let b = val(&buff).to_bytes();
            let rx = crate::perl::regex::compile(br"\x55\x26\x11\x05\0", "").expect("pattern");
            let end = match rx.match_at(&b, 0) {
                Ok(Some(sp)) => sp[0].map(|(_, e)| e),
                _ => None,
            };
            found = match end {
                Some(e) => {
                    pos = pl::sub(&pl::add(&trailer_start, &n(e as i64)), &n(5));
                    pos.truthy()
                }
                None => false,
            };
        }
        found = found && pl::method(it, &raf, "Seek", vec![pos.clone(), n(0)])?.truthy() && pl::num_eq(&raf_read_cell(it, &raf, &buff, hd_len.clone())?, &hd_len);
        if !found {
            pl::method(it, &et, "Warn", vec![pl::s("Error reading HiddenData"), n(1)])?;
            return pl::ret_undef();
        }
        it.set_arg(1, pos)?; // return fixed offset
        if pl::method(it, &et, "Options", vec![pl::s("Validate")])?.truthy() || et.get("IsWriting").truthy() {
            pl::method(it, &et, "Warn", vec![pl::s("Fixed incorrect HiddenDataOffset"), n(1)])?;
        }
    }
    ret(rf(&buff))
}

//------------------------------------------------------------------------------
/// Process "SONY PIC\0" maker notes (DSC-H200/J10/W370/W510, MHS-TS20, ref PH)
/// Inputs: 0) ExifTool object ref, 1) dirInfo ref, 2) tag table ref
/// Returns: 1
pub fn process_sony_pic(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info, tag_table_ptr) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    let data_pt = dir_info.get("DataPt");
    let start = or(dir_info.get("DirStart"), n(0));
    let len = or(dir_info.get("DirLen"), pl::sub(&pl::length(&data_pt.deref()), &start));
    let data = substr(it, &data_pt.deref(), &start, Some(&len))?;

    // H200 panorama images have an IFD at offset 12 (non-panoramas have 0's here)
    // - assume other images could too, but do a bit of validation to check
    // - MHS-TS20 images have some other data here
    if pl::ge(&len, &n(26)) {
        let mut count = g16u(it, &data_pt, pl::add(&start, &n(12)))?;
        if pl::gt(&count, &n(256)) {
            pl::call(it, "Image::ExifTool::ToggleByteOrder", vec![])?;
            count = g16u(it, &data_pt, pl::add(&start, &n(12)))?;
        }
        if count.truthy() && pl::lt(&count, &n(256)) {
            let format = g16u(it, &data_pt, pl::add(&start, &n(16)))?;
            if pl::ge(&format, &n(1)) && pl::le(&format, &n(10)) {
                dir_info.set("DirStart", pl::add(&start, &n(12)));
                dir_info.set("DirLen", pl::sub(&len, &n(12)));
                let sony_table = get_tag_table(it, "Image::ExifTool::Sony::Main")?;
                pl::call(it, "Image::ExifTool::Exif::ProcessExif", vec![et.clone(), dir_info.clone(), sony_table])?;
            }
        }
    }
    // Do a brute force search for text data:
    // For the DSC-J10/W370/W510 the first text block is at offset 0x1ec and
    // starts with "BarCode:".  For the H200 it is at 0x1f0 and starts with "BC:".
    // For the TS20 it is at 0x5b and starts with "V400 AELOG\nbarcode:".
    // The second text block starts with "AFLOG" (Auto-Focus log) and is at
    // 0x600 for all models, except for the TS20 it is at 0x45b.
    let mut i = 0i64;
    for m in re(r"(\w[\x09\x0a\x0d\x20-\x7e]+)", "s").each(&data) {
        let c1 = cap(&m, 1);
        if !pl::gt(&pl::length(&c1), &n(32)) {
            continue;
        }
        i += 1;
        let (tag, v) = (pl::s(&format!("TextInfo{i}")), c1);
        if !tag_table_ptr.get(tag.to_bytes()).truthy() {
            let ti = Value::new_hash();
            ti.set("Name", tag.clone());
            ti.set("Binary", n(1));
            pl::call(it, "Image::ExifTool::AddTagToTable", vec![tag_table_ptr.clone(), tag.clone(), ti])?;
        }
        pl::method(it, &et, "HandleTag", vec![tag_table_ptr.clone(), tag, v.clone()])?;
        // extract interesting tags separately (might want to speed this up)
        let mut keys: Vec<Value> = tag_table_ptr.keys().into_iter().map(Value::from).collect();
        keys.sort_by(|x, y| pl::lc(x).to_bytes()[..].cmp(&pl::lc(y).to_bytes()[..]));
        for tag in keys {
            if !re(r":$", "").is_match(&tag) {
                continue;
            }
            let mut p = br"\b".to_vec();
            p.extend_from_slice(&tag.to_bytes());
            p.extend_from_slice(br"\s*([^\s;,:]+)");
            let Some(m2) = pl::re_dyn(&p, "")?.m(&v) else { continue };
            pl::method(it, &et, "HandleTag", vec![tag_table_ptr.clone(), tag, cap(&m2, 1)])?;
        }
    }
    ret(n(1))
}

//------------------------------------------------------------------------------
/// MeterInfo value conversions
/// Inputs: 0) value
/// Returns: converted value
pub fn conv_meter1(_it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let v = arg(&a, 0);
    if pl::length(&v).to_i64() != 90 || v.is_undef() {
        return ret(pl::sref(v));
    }
    let av = pl::unpack("SLLSLLSLLSLLSLLSLLSLLSLLSLL", &v)?;
    ret(pl::join(" ", &av))
}

pub fn conv_meter2(_it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let v = arg(&a, 0);
    if pl::length(&v).to_i64() != 110 || v.is_undef() {
        return ret(pl::sref(v));
    }
    let av = pl::unpack("SLLSLLSLLSLLSLLSLLSLLSLLSLLSLLSLL", &v)?;
    ret(pl::join(" ", &av))
}

//------------------------------------------------------------------------------
/// LensSpec value conversions
/// Inputs: 0) value
/// Returns: converted value
/// Notes: unpacks in format compatible with LensInfo, with extra flags bytes at start and end
pub fn conv_lens_spec(_it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let v = arg(&a, 0);
    if v.is_undef() || pl::length(&v).to_i64() != 8 {
        return ret(pl::sref(v));
    }
    let mut av = pl::unpack("H2H4H4H2H2H2", &v)?;
    av.resize(av.len().max(6), Value::Undef);
    av[1] = pl::add(&av[1], &n(0)); // remove leading zeros from focal lengths
    av[2] = pl::add(&av[2], &n(0));
    for i in [3, 4] {
        // convert hex digits (ie. "b0" = f11)
        re("([a-f])", "").subst(&mut av[i], false, |c| hex(&cap(c, 1)).to_bytes().to_vec());
    }
    av[3] = pl::div(&av[3], &n(10))?; // divide f-numbers by 10
    av[4] = pl::div(&av[4], &n(10))?;
    ret(pl::join(" ", &av))
}

pub fn conv_inv_lens_spec(_it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let v = arg(&a, 0);
    let mut av = pl::split_ws(&v);
    if av.len() != 6 {
        return ret(v);
    }
    av[3] = pl::mul(&av[3], &n(10)); // f-numbers are multiplied by 10
    av[4] = pl::mul(&av[4], &n(10));
    for i in [3, 4] {
        re(r"^(\d{2})0$", "").subst(&mut av[i], false, |c| pl::sprintf("%x0", &[cap(c, 1)]).to_bytes().to_vec());
    }
    for x in av.iter_mut() {
        *x = hex(x); // convert from hex
    }
    ret(pl::pack("CnnCCC", &av)?)
}

//------------------------------------------------------------------------------
/// Print Sony LensSpec value
/// Inputs: 0) LensSpec numerical value
/// Returns: converted LensSpec string (eg. "DT 18-55mm F3.5-5.6 SAM")
/// Refs: http://equational.org/importphotos/alphalensinfo.html
///       http://www.dyxum.com/dforum/the-lens-information-different-from-lensid_topic37682.html
pub fn print_lens_spec(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let v = arg(&a, 0);
    let mut rtn_val = Value::Undef;
    let (mut f1, mut f2) = (Value::Undef, Value::Undef);
    // 0=flags1, 1=short focal, 2=long focal, 3=max aperture at short focal,
    // 4=max aperture at long focal, 5=flags2
    let av = pl::split_ws(&v);
    if av.len() == 2 {
        // LensSpecFeatures patch
        f1 = arg(&av, 0);
        f2 = arg(&av, 1);
        rtn_val = pl::s("");
    } else if av.len() >= 6 {
        f1 = arg(&av, 0);
        let (mut sf, lf, mut sa, la) = (arg(&av, 1), arg(&av, 2), arg(&av, 3), arg(&av, 4));
        f2 = arg(&av, 5);
        // crude validation of focal length and aperture values
        if !pl::num_eq(&sf, &n(0)) && !pl::num_eq(&sa, &n(0)) && (pl::num_eq(&lf, &n(0)) || pl::ge(&lf, &sf)) && (pl::num_eq(&la, &n(0)) || pl::ge(&la, &sa)) {
            // use focal and aperture range if this is a zoom lens
            if !pl::num_eq(&lf, &sf) && !pl::num_eq(&lf, &n(0)) {
                sf = pl::concat(&sf, &pl::concat(&pl::s("-"), &lf));
            }
            if !pl::num_eq(&sa, &la) && !pl::num_eq(&la, &n(0)) {
                sa = pl::concat(&sa, &pl::concat(&pl::s("-"), &la));
            }
            rtn_val = pl::concat(&pl::concat(&sf, &pl::s("mm F")), &sa); // heart of LensSpec is a LensInfo string
        }
    }
    if !rtn_val.is_undef() {
        // loop through available lens features
        let flags = hex(&pl::concat(&f1, &f2));
        let lens_features = crate::runtime::lex_array(it, MODULE, "lensFeatures");
        for feature in lens_features.items() {
            let bits = ops::bit_and(&feature.at(0), &flags);
            let name = feature.at(1).get(bits.to_bytes());
            if !(bits.truthy() || name.truthy()) {
                continue;
            }
            // add feature name as a prefix or suffix to the LensSpec
            let s = if name.truthy() { name } else { pl::sprintf("Unknown(%.4x)", &[bits]) };
            rtn_val = if rtn_val.truthy() {
                if feature.at(2).truthy() {
                    pl::concat(&pl::concat(&s, &pl::s(" ")), &rtn_val)
                } else {
                    pl::concat(&pl::concat(&rtn_val, &pl::s(" ")), &s)
                }
            } else {
                s
            };
        }
    } else {
        rtn_val = pl::concat(&pl::concat(&pl::s("Unknown ("), &v), &pl::s(")"));
    }
    ret(rtn_val)
}

/// inverse conversion
pub fn print_inv_lens_spec(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (v, features) = (arg(&a, 0), arg(&a, 2));
    if let Some(m) = re(r"Unknown \((.*)\)", "i").m(&v) {
        return ret(cap(&m, 1));
    }
    let li = pl::call_list(it, "Image::ExifTool::Exif::GetLensInfo", vec![v.clone()])?;
    let (sf, mut lf, sa, mut la) = (arg(&li, 0), arg(&li, 1), arg(&li, 2), arg(&li, 3));
    let s;
    if features.truthy() {
        s = pl::s("");
    } else if sf.truthy() {
        // fixed focal length and aperture have zero for 2nd number
        if pl::num_eq(&lf, &sf) {
            lf = n(0);
        }
        if pl::num_eq(&la, &sa) {
            la = n(0);
        }
        s = pl::concat(&pl::s(" "), &pl::join(" ", &[sf, lf, sa, la]));
    } else {
        return pl::ret_undef();
    }
    let mut flags = n(0);
    let lens_features = crate::runtime::lex_array(it, MODULE, "lensFeatures");
    for feature in lens_features.items() {
        let h = feature.at(1);
        for bits in h.keys() {
            // set corresponding flag bits for each feature name found
            let name = h.get(&bits);
            let mut p = br"\b".to_vec();
            p.extend_from_slice(&name.to_bytes());
            p.extend_from_slice(br"\b");
            if pl::re_dyn(&p, "i")?.is_match(&v) {
                flags = ops::bit_or(&flags, &Value::from(bits.clone()));
            }
        }
    }
    let fmt = pl::concat(&pl::concat(&pl::s("%.2x"), &s), &pl::s(" %.2x"));
    ret(pl::sprintf_v(&fmt, &[ops::shift_right(&flags, &n(8)), ops::bit_and(&flags, &n(0xff))]))
}

//------------------------------------------------------------------------------
/// Read/Write MoreInfo information (tag 0x0020, count 20480)
/// Inputs: 0) ExifTool object ref, 1) dirInfo ref, 2) tag table ref
/// Returns: 1 on success when reading, or new directory when writing (IsWriting set)
pub fn process_more_info(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info, tag_table_ptr) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    if !et.truthy() {
        return ret(n(1)); // allow dummy access to write routine
    }
    let data_pt = dir_info.get("DataPt");
    let start = or(dir_info.get("DirStart"), n(0));
    let dir_len = or(dir_info.get("DirLen"), pl::length(&data_pt.deref()));
    let is_writing = dir_info.get("IsWriting");
    let rtn_val0 = if is_writing.truthy() { Value::Undef } else { n(0) };
    if pl::lt(&dir_len, &n(4)) {
        return ret(rtn_val0);
    }

    let num = g16u(it, &data_pt, start.clone())?; // number of entries
    let mut len = g16u(it, &data_pt, pl::add(&start, &n(2)))?; // total data length

    if pl::lt(&dir_len, &pl::add(&n(4), &pl::mul(&num, &n(4)))) {
        pl::method(it, &et, "Warn", vec![pl::s("Truncated MoreInfo data"), n(1)])?;
        return ret(rtn_val0);
    }
    if pl::gt(&num, &n(50)) {
        pl::method(it, &et, "Warn", vec![pl::s("Possibly corrupted MoreInfo data"), n(1)])?;
        return ret(rtn_val0);
    }

    if !is_writing.truthy() {
        pl::method(it, &et, "VerboseDir", vec![pl::s("MoreInfo"), num.clone(), len.clone()])?;
    }

    if pl::gt(&len, &dir_len) {
        pl::method(it, &et, "Warn", vec![pl::s("MoreInfo data length too large"), n(1)])?;
        len = dir_len.clone();
    }
    // loop through the MoreInfo index section to get the block offsets and tag ID's
    // (in case they are out of order, even though this may never happen)
    let num_i = num.to_i64();
    let (mut offset, mut tag_id): (Vec<Value>, Vec<Value>) = (Vec::new(), Vec::new());
    let block_size = Value::new_hash();
    for i in 0..num_i {
        let entry = pl::add(&pl::add(&start, &n(4)), &n(i * 4));
        tag_id.push(g16u(it, &data_pt, entry.clone())?);
        offset.push(g16u(it, &data_pt, pl::add(&entry, &n(2)))?);
        let last = offset.last().unwrap();
        if pl::gt(last, &len) && pl::le(last, &dir_len) {
            pl::method(it, &et, "Warn", vec![pl::s("MoreInfo data length too small"), n(1)])?;
            len = dir_len.clone();
        }
    }
    // generate a lookup table of block sizes
    let mut sorted = offset.clone();
    sorted.sort_by(|x, y| ops::num_cmp(x, y).unwrap_or(std::cmp::Ordering::Equal));
    sorted.push(n(0xffff)); // (simplifies logic in loop below)
    for i in 0..num_i as usize {
        let off = sorted[i].clone();
        let mut size = pl::sub(&sorted[i + 1], &off);
        // note that block size will be negative for blocks with starting
        // offsets greater than $dirLen, but we will ignore these below
        if pl::gt(&size, &pl::sub(&len, &off)) {
            size = pl::sub(&len, &off);
        }
        // (if blockSize is already defined for this offset, then there
        //  are 2 blocks with the same starting offset and the existing
        //  size must be zero.  Since we can't know which block is
        //  actually non-zero size, the reasonable thing to do is
        //  assume that both have a size of zero)
        if block_size.get(off.to_bytes()).is_undef() {
            block_size.set(off.to_bytes(), size);
        }
    }
    // initialize successful return value
    let rtn_val = cell(if is_writing.truthy() { substr(it, &data_pt.deref(), &start, Some(&dir_len))? } else { n(1) });
    // now process each block
    let unknown = et.get("OPTIONS").get("Unknown");
    for i in 0..num_i as usize {
        if pl::gt(&offset[i], &dir_len) {
            continue; // ignore bad offsets
        }
        let tag = tag_id[i].clone();
        if is_writing.truthy() {
            // write new tags
            let tag_info = tag_table_ptr.get(tag.to_bytes());
            if !(pl::ref_of(&tag_info) == "HASH" && tag_info.get("SubDirectory").truthy()) {
                continue;
            }
            let off = offset[i].clone();
            let size = block_size.get(off.to_bytes());
            if !size.truthy() {
                continue; // ignore zero-length blocks
            }
            let sub_dir_info = Value::new_hash();
            sub_dir_info.set("DirName", tag_info.get("Name"));
            sub_dir_info.set("Parent", dir_info.get("DirName"));
            sub_dir_info.set("DataPt", rf(&rtn_val));
            sub_dir_info.set("DirStart", off.clone());
            sub_dir_info.set("DirLen", size.clone());
            let sub_table = pl::call(it, "Image::ExifTool::GetTagTable", vec![tag_info.get("SubDirectory").get("TagTable")])?;
            let v = pl::method(it, &et, "WriteDirectory", vec![sub_dir_info, sub_table])?;
            // update this block in the returned MoreInfo data
            if !v.is_undef() {
                let nv = substr_assign(&val(&rtn_val), &off, &size, &v)?;
                *rtn_val.borrow_mut() = nv;
            }
            continue;
        }
        // generate binary tables for unknown tags if -U option used
        if tag_table_ptr.get(tag.to_bytes()).is_undef() && pl::gt(&unknown, &n(1)) {
            let name = pl::sprintf("MoreInfo%.4x", &[tag.clone()]);
            let table = pl::concat(&pl::s("Image::ExifTool::Sony::"), &name);
            let t = pl::pkg_hash(it, &table.to_text());
            if let Value::HashRef(h) = &t {
                h.borrow_mut().clear();
            }
            t.set("PROCESS_PROC", Value::Code(it.sub_ref("Image::ExifTool::ProcessBinaryData")));
            t.set("FIRST_ENTRY", n(0));
            let groups = Value::new_hash();
            groups.set("0", pl::s("MakerNotes"));
            groups.set("2", pl::s("Image"));
            t.set("GROUPS", groups);
            let ti = Value::new_hash();
            ti.set("Name", name);
            let sd = Value::new_hash();
            sd.set("TagTable", table);
            ti.set("SubDirectory", sd);
            // tag must not be deleted because new table will be created
            //  but the old table was already loaded
            ti.set("NeverDelete", n(1));
            pl::call(it, "Image::ExifTool::AddTagToTable", vec![tag_table_ptr.clone(), tag.clone(), ti])?;
        }
        pl::method(
            it,
            &et,
            "HandleTag",
            vec![
                tag_table_ptr.clone(),
                tag,
                Value::Undef,
                pl::s("Index"),
                n(i as i64),
                pl::s("DataPt"),
                data_pt.clone(),
                pl::s("DataPos"),
                dir_info.get("DataPos"),
                pl::s("Start"),
                pl::add(&start, &offset[i]),
                pl::s("Size"),
                block_size.get(offset[i].to_bytes()),
            ],
        )?;
    }
    ret(val(&rtn_val))
}

//------------------------------------------------------------------------------
/// Read Sony DSC-F1 PMP file
/// Inputs: 0) ExifTool object ref, 1) dirInfo ref
/// Returns: 1 on success when reading, 0 if this isn't a valid PMP file
pub fn process_pmp(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info) = (arg(&a, 0), arg(&a, 1));
    let raf = dir_info.get("RAF");
    let buff = cell(Value::Undef);
    if !pl::num_eq(&raf_read_cell(it, &raf, &buff, n(128))?, &n(128)) {
        return ret(n(0));
    }
    // validate header length (124 bytes)
    if !re(r"^.{8}\0{3}\x7c.{112}\xff\xd8\xff\xdb$", "s").is_match(&val(&buff)) {
        return ret(n(0));
    }
    pl::method(it, &et, "SetFileType", vec![])?;
    pl::call(it, "Image::ExifTool::SetByteOrder", vec![pl::s("MM")])?;
    pl::method(it, &et, "FoundTag", vec![pl::s("Make"), pl::s("Sony")])?;
    pl::method(it, &et, "FoundTag", vec![pl::s("Model"), pl::s("DSC-F1")])?;
    // extract information from 124-byte header
    let tag_table_ptr = get_tag_table(it, "Image::ExifTool::Sony::PMP")?;
    let d = Value::new_hash();
    d.set("DataPt", rf(&buff));
    d.set("DirName", pl::s("PMP"));
    pl::method(it, &et, "ProcessDirectory", vec![d, tag_table_ptr])?;
    // process JPEG image
    pl::method(it, &raf, "Seek", vec![n(124), n(0)])?;
    dir_info.set("Base", n(124));
    pl::method(it, &et, "ProcessJPEG", vec![dir_info])?;
    ret(n(1))
}

//------------------------------------------------------------------------------
/// Set the ARW file type and decide between SubIFD and A100DataOffset
/// Inputs: 0) ExifTool object ref, 1) reference to tag 0x14a raw data
/// Returns: true if tag 0x14a is a SubIFD, false otherwise
pub fn set_arw(it: &mut Interp, a: Vec<Value>, want: Want) -> ER<Vec<Value>> {
    let (et, val_pt) = (arg(&a, 0), arg(&a, 1));

    // assume ARW for now -- SR2's get identified when FileFormat is parsed
    et.set("TIFF_TYPE", pl::s("ARW"));
    pl::method(it, &et, "OverrideFileType", vec![pl::s("ARW")])?;

    // this should always be a SubIFD for models other than the A100
    if !(pl::str_eq(&et.get("Model"), "DSLR-A100") && pl::length(&deref_strict(&val_pt)?).to_i64() == 4) {
        return ret(n(1));
    }

    // for the A100, IFD0 tag 0x14a is either a pointer to the raw data if this is
    // an original image, or a SubIFD offset if the image was edited by Sony IDC,
    // so assume it points to the raw data if it isn't a valid IFD (this assumption
    // will be checked later when we try to parse the SR2Private directory)
    let subdir = Value::new_hash();
    subdir.set("DirStart", g32u(it, &val_pt, n(0))?);
    subdir.set("Base", n(0));
    subdir.set("RAF", et.get("RAF"));
    subdir.set("AllowOutOfOrderTags", n(1)); // doh!
    it.call_named("Image::ExifTool::Exif::ValidateIFD", vec![subdir], want)
}

//------------------------------------------------------------------------------
/// Finish writing ARW image, patching necessary Sony quirks, etc
/// Inputs: 0) ExifTool ref, 1) dirInfo ref, 2) EXIF data ref, 3) image data reference
/// Returns: undef on success, error string otherwise
/// Notes: (it turns that all of this is for the A100 only)
pub fn finish_arw(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info, data_pt, image_data) = (arg(&a, 0), arg(&a, 1), arg(&a, 2), arg(&a, 3));

    // pre-scan IFD0 to get IFD entry offsets for each tag
    let data_len = pl::length(&deref_strict(&data_pt)?);
    if pl::lt(&data_len, &n(2)) {
        return ret(pl::s("Truncated IFD0"));
    }
    let nn = g16u(it, &data_pt, n(0))?;
    if pl::lt(&data_len, &pl::add(&n(2), &pl::mul(&n(12), &nn))) {
        return ret(pl::s("Truncated IFD0"));
    }
    let entry = Value::new_hash();
    let (mut pad, mut data_offset) = (Value::Undef, Value::Undef);
    let mut i = n(0);
    while pl::lt(&i, &nn) {
        let e = pl::add(&n(2), &pl::mul(&i, &n(12)));
        let id = g16u(it, &data_pt, e.clone())?;
        entry.set(id.to_bytes(), e);
        i = pl::add(&i, &n(1));
    }
    let ent = |id: i64| entry.get(n(id).to_bytes());
    // fix up SR2Private offset and A100DataOffset (A100 only)
    if ent(0xc634).truthy() && et.get("MRWDirData").truthy() {
        if !pl::str_eq(&et.get("Model"), "DSLR-A100") {
            return ret(pl::s("Unexpected MRW block"));
        }
        if !(ent(0x14a).truthy() && et.get("A100DataOffset").truthy()) {
            return ret(pl::s("Missing A100DataOffset"));
        }
        // account for total length of image data
        let mut total_len = pl::add(&n(8), &data_len);
        if image_data.is_ref() {
            for data_block in image_data.items() {
                let (size, pad) = (data_block.at(1), data_block.at(2));
                total_len = pl::add(&total_len, &pl::add(&size, &pad));
            }
        }
        // align MRW block on an even 4-byte boundary
        let mut remain = ops::bit_and(&total_len, &n(0x03));
        if remain.truthy() {
            pad = pl::sub(&n(4), &remain);
            if pad.truthy() {
                total_len = pl::add(&total_len, &pad);
            }
        }
        // set offset for the MRW directory data
        s32u_at(it, total_len.clone(), &data_pt, pl::add(&ent(0xc634), &n(8)))?;
        // also pad MRWDirData data to an even 4 bytes (just to be safe)
        remain = ops::bit_and(&pl::length(&et.get("MRWDirData")), &n(0x03));
        if remain.truthy() {
            let cnt = pl::sub(&n(4), &remain).to_i64().max(0) as usize;
            et.set("MRWDirData", pl::concat(&et.get("MRWDirData"), &Value::bytes(vec![0u8; cnt])));
        }
        total_len = pl::add(&total_len, &pl::length(&et.get("MRWDirData")));
        // fix up A100DataOffset
        data_offset = et.get("A100DataOffset");
        s32u_at(it, total_len, &data_pt, pl::add(&ent(0x14a), &n(8)))?;
    }
    // patch double-referenced and incorrectly-sized A100 PreviewImage
    if ent(0x201).truthy() && et.get("A100PreviewStart").truthy() && ent(0x202).truthy() && et.get("A100PreviewLength").truthy() {
        s32u_at(it, et.get("A100PreviewStart"), &data_pt, pl::add(&ent(0x201), &n(8)))?;
        s32u_at(it, et.get("A100PreviewLength"), &data_pt, pl::add(&ent(0x202), &n(8)))?;
    }
    // write TIFF IFD structure
    let outfile = dir_info.get("OutFile");
    let bo = pl::call(it, "Image::ExifTool::GetByteOrder", vec![])?;
    let h16 = first(part2::set16u(it, vec![n(0x2a)], Want::Scalar))?;
    let h32 = s32u(it, n(8))?;
    let header = pl::concat(&pl::concat(&bo, &h16), &h32);
    if !pl::call(it, "Image::ExifTool::Write", vec![outfile.clone(), header, data_pt.deref()])?.truthy() {
        return ret(pl::s("Error writing"));
    }
    // copy over image data
    if image_data.is_ref() && !pl::method(it, &et, "CopyImageData", vec![image_data, outfile.clone()])?.truthy() {
        return ret(pl::s("Error copying image data"));
    }
    // write MRW data if necessary
    if et.get("MRWDirData").truthy() {
        if pad.truthy() {
            let cnt = pad.to_i64().max(0) as usize;
            pl::call(it, "Image::ExifTool::Write", vec![outfile.clone(), Value::bytes(vec![0u8; cnt])])?; // write padding if necessary
        }
        pl::call(it, "Image::ExifTool::Write", vec![outfile, et.get("MRWDirData")])?;
        et.delete("MRWDirData");
        // set TIFF_END to copy over the MRW image data
        if data_offset.truthy() {
            et.set("TIFF_END", data_offset);
        }
    }
    pl::ret_undef()
}

//------------------------------------------------------------------------------
/// Decrypt/Encrypt Sony data (ref 1) (reversible encryption)
/// Inputs: 0) data reference, 1) start offset, 2) data length, 3) decryption key
/// Returns: nothing (original data buffer is updated with decrypted data)
/// Notes: data length should be a multiple of 4
pub fn decrypt(_it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (data_pt, start, len, key) = (arg(&a, 0), arg(&a, 1), arg(&a, 2), arg(&a, 3));
    decrypt_rs(&data_pt, &start, &len, &key)?;
    Ok(vec![])
}

fn decrypt_rs(data_pt: &Value, start: &Value, len: &Value, key: &Value) -> ER<()> {
    let mut pad = [0u64; 128];
    let words = pl::int(&pl::div(len, &n(4))?);

    // (the key goes through '&' and '>>' first, which take it as an unsigned integer)
    let mut key = ops::to_uv(key);
    for p in pad.iter_mut().take(4) {
        let lo = (key & 0xffff) * 0x0edd + 1;
        let hi = (key >> 16) * 0x0edd + (key & 0xffff) * 0x02e9 + (lo >> 16);
        key = ((hi & 0xffff) << 16) + (lo & 0xffff);
        *p = key;
    }
    pad[3] = (pad[3] << 1 | (pad[0] ^ pad[2]) >> 31) & 0xffffffff;
    for i in 4..0x7f {
        pad[i] = ((pad[i - 4] ^ pad[i - 2]) << 1 | (pad[i - 3] ^ pad[i - 1]) >> 31) & 0xffffffff;
    }
    let tmpl = pl::concat(&pl::concat(&pl::concat(&pl::s("x"), start), &pl::s(" N")), &words);
    let mut data: Vec<u64> = match crate::perl::pack::unpack(&tmpl.to_bytes(), &deref_strict(data_pt)?.to_bytes()) {
        Ok(v) => v.iter().map(ops::to_uv).collect(),
        Err(e) => return die(e),
    };
    let nw = words.to_i64().max(0) as usize;
    if data.len() < nw {
        data.resize(nw, 0); // (undef ^ pad, as perl)
    }
    let mut i = 0x7f;
    for d in data.iter_mut().take(nw) {
        pad[i & 0x7f] = pad[(i + 1) & 0x7f] ^ pad[(i + 65) & 0x7f];
        *d ^= pad[i & 0x7f];
        i += 1;
    }
    let packed: Vec<Value> = data.iter().map(|&x| ops::from_uv(x)).collect();
    let packed = pl::pack("N*", &packed)?;
    let nv = substr_assign(&data_pt.deref(), start, &pl::mul(&words, &n(4)), &packed)?;
    pl::set_deref(data_pt, nv)
}

/// The enciphered values of the bytes 0x02 to 0xf7, as in the Perl's tr///
/// (`$c = ($b*$b*$b) % 249`).
const CIPHER: &[u8] = b"\x08\x1b\x40\x7d\xd8\x5e\x0e\xe7\x04V\xea\xcd\x05\x8ap\xb6i\x88\x200\xbe\xd7\x81\xbb\x92\x0c\x28\xecl\xa0\x95Q\xd3\x2f\x5dj\x5c9\x07\xc5\x87L\x1a\xf0\xe2\xef\x24y\x02\xb7\xac\xe0\x60\x2bG\xba\x91\xcbu\x8e\x233\xc4\xe3\x96\xdc\xc2N\x7fb\xf6OeE\xeet\xcf\x138KRST\x5bn\x93\xd02\xb1aAW\xa9D\x27X\xdd\xc3\x10\xbc\xdbs\x83\x181\xd4\x15\xe5_\x7bF\xbf\xf3\xe8\xa4\x2d\x82\xb0\xbd\xaf\x8cZ\x1f\xda\x9fmJ\x3cIw\xccU\x11\x06\x3a\xb3\x7e\x9a\x14\xe4\x25\xc8\xe1v\x86\x1e\x3d\xe96\x1c\xa1\xd2\xb5P\xa2\xb8\x98H\xc7\x29f\x8b\x9e\xa5\xa6\xa7\xae\xc1\xe6\x2a\x85\x0b\xb4\x94\xaa\x03\x97z\xab7\x1dc\x165\xc6\xd6k\x84\x2eh\x3f\xb2\xce\x99\x19MB\xf7\x80\xd5\x0a\x17\x09\xdf\xadr4\xf2\xc0\x9d\x8f\x9c\xca\x26\xa8dY\x8d\x0d\xd1\xedg\x3ex\x22\x3b\xc9\xd9q\x90C\x89o\xf4\x2c\x0f\xa3\xf5\x12\xeb\x9b\x21\x7c\xb9\xde\xf1";

/// The tr/// table of one direction: (byte map, which bytes are in the search list).
fn cipher_table(encipher: bool) -> ([u8; 256], [bool; 256]) {
    let mut map = [0u8; 256];
    let mut hit = [false; 256];
    for (k, &c) in CIPHER.iter().enumerate() {
        let plain = 2 + k as u8;
        let (from, to) = if encipher { (plain, c) } else { (c, plain) };
        if !hit[from as usize] {
            // (the first occurrence in the search list wins, as tr)
            hit[from as usize] = true;
            map[from as usize] = to;
        }
    }
    (map, hit)
}

/// `$$dataPt =~ tr/.../.../`: returns the count, as tr.
fn decipher_rs(data_pt: &Value, encipher: bool) -> ER<Value> {
    thread_local!(static TABLES: [([u8; 256], [bool; 256]); 2] = [cipher_table(false), cipher_table(true)]);
    let v = deref_strict(data_pt)?;
    if v.is_undef() {
        return Ok(n(0)); // (tr leaves an undefined value as it is)
    }
    let b = v.to_bytes();
    let mut out = b.to_vec();
    let mut count = 0i64;
    TABLES.with(|t| {
        let (map, hit) = &t[encipher as usize];
        for x in out.iter_mut() {
            if hit[*x as usize] {
                *x = map[*x as usize];
                count += 1;
            }
        }
    });
    pl::set_deref(data_pt, Value::bytes(out))?;
    Ok(n(count))
}

//------------------------------------------------------------------------------
/// Decipher/encipher Sony tag 0x2010, 0x900b, 0x9050 and 0x940x data (ref PH)
/// Inputs: 0) data reference, 1) true to encipher the data
pub fn decipher(_it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (data_pt, encipher) = (arg(&a, 0), arg(&a, 1));
    // This is a simple substitution cipher, so use a hardcoded translation table for speed.
    // The formula is: $c = ($b*$b*$b) % 249, where $c is the enciphered data byte
    // (note that bytes with values 249-255 are not translated, and 0-1, 82-84,
    //  165-167 and 248 have the same enciphered value)
    ret(decipher_rs(&data_pt, encipher.truthy())?)
}

//------------------------------------------------------------------------------
/// Process Sony 0x94xx cipherdata directory
/// Inputs: 0) ExifTool object ref, 1) directory information ref, 2) tag table ref
/// Returns: 1 on success
/// Notes:
/// 1) dirInfo may contain VarFormatData (reference to empty list) to return
///    details about any variable-length-format tags in the table (used when writing)
/// 2) A bug in ExifTool 9.04-9.10 could have double-enciphered these blocks
pub fn process_enciphered(it: &mut Interp, a: Vec<Value>, want: Want) -> ER<Vec<Value>> {
    let (et, dir_info, tag_table_ptr) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    let data_pt = dir_info.get("DataPt");
    let dir_start = or(dir_info.get("DirStart"), n(0));
    let dir_len = or(dir_info.get("DirLen"), pl::sub(&pl::length(&data_pt.deref()), &dir_start));
    let data = cell(substr(it, &data_pt.deref(), &dir_start, Some(&dir_len))?);
    let new_dir_info = dir_info.hash_copy();
    new_dir_info.set("DataPt", rf(&data));
    new_dir_info.set("DataPos", pl::add(&dir_info.get("DataPos"), &dir_start));
    new_dir_info.set("DirStart", n(0));
    decipher_rs(&rf(&data), false)?;
    if et.get("DoubleCipher").truthy() {
        decipher_rs(&rf(&data), false)?;
        pl::method(it, &et, "Warn", vec![pl::s("Some Sony metadata is double-enciphered. Write any tag to fix"), n(1)])?;
    }
    if pl::gt(&pl::method(it, &et, "Options", vec![pl::s("Verbose")])?, &n(2)) {
        let mut tag_info = dir_info.get("TagInfo");
        if !tag_info.truthy() {
            tag_info = Value::new_hash();
            tag_info.set("Name", pl::s("data"));
        }
        let s = if et.get("DoubleCipher").truthy() { "ouble-d" } else { "" };
        let title = pl::concat(&pl::s(&format!("D{s}eciphered ")), &tag_info.get("Name"));
        pl::method(it, &et, "VerboseDir", vec![title])?;
        let data_pos = pl::add(&pl::add(&dir_info.get("DirStart"), &dir_info.get("DataPos")), &or(dir_info.get("Base"), n(0)));
        pl::method(it, &et, "VerboseDump", vec![rf(&data), pl::s("Prefix"), pl::concat(&et.get("INDENT"), &pl::s("  ")), pl::s("DataPos"), data_pos])?;
    }
    it.call_method(&et, "ProcessBinaryData", vec![et.clone(), new_dir_info, tag_table_ptr], want)
}

//------------------------------------------------------------------------------
/// Write Sony 0x94xx cipherdata directory
/// Inputs: 0) ExifTool object ref, 1) source dirInfo ref, 2) tag table ref
/// Returns: cipherdata block or undefined on error
pub fn write_enciphered(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info, tag_table_ptr) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    if !et.truthy() {
        return ret(n(1));
    }
    let data_pt = dir_info.get("DataPt");
    let dir_start = or(dir_info.get("DirStart"), n(0));
    let dir_len = or(dir_info.get("DirLen"), pl::sub(&pl::length(&data_pt.deref()), &dir_start));
    let data = cell(substr(it, &data_pt.deref(), &dir_start, Some(&dir_len))?);
    let changed = et.get("CHANGED");
    decipher_rs(&rf(&data), false)?;
    // fix double-enciphered data (due to bug in ExifTool 9.04-9.10)
    if et.get("DoubleCipher").truthy() {
        decipher_rs(&rf(&data), false)?;
        et.set("CHANGED", pl::add(&et.get("CHANGED"), &n(1)));
        pl::method(it, &et, "Warn", vec![pl::s("Fixed double-enciphered Sony metadata"), n(1)])?;
    }
    let new_dir_info = dir_info.hash_copy();
    new_dir_info.set("DataPt", rf(&data));
    new_dir_info.set("DataPos", pl::add(&dir_info.get("DataPos"), &dir_start));
    new_dir_info.set("DirStart", n(0));
    let w = pl::method(it, &et, "WriteBinaryData", vec![new_dir_info, tag_table_ptr])?;
    *data.borrow_mut() = w;
    if pl::num_eq(&changed, &et.get("CHANGED")) {
        // nothing changed, so recover original data
        let orig = substr(it, &data_pt.deref(), &dir_start, Some(&dir_len))?;
        *data.borrow_mut() = orig;
    } else if !val(&data).is_undef() {
        decipher_rs(&rf(&data), true)?; // re-encipher
    }
    ret(val(&data))
}

//------------------------------------------------------------------------------
/// Process "rtmd" timed metadata embedded in Sony MP4 videos (ref PH)
/// Inputs: 0) ExifTool object ref, 1) dirInfo ref, 2) tag table ref
/// Returns: 1 on success
pub fn process_rtmd(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info, tag_table_ptr) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    let data_pt = dir_info.get("DataPt");
    let data_pos = pl::add(&or(dir_info.get("DataPos"), n(0)), &or(dir_info.get("Base"), n(0)));
    let end = pl::length(&data_pt.deref());
    if pl::lt(&end, &n(2)) {
        return ret(n(0));
    }
    pl::method(it, &et, "VerboseDir", vec![pl::s("Sony rtmd"), Value::Undef, end.clone()])?;
    // Note:  The 0x1c-byte header contains some as-yet unextracted information:
    //  offset 0x0e: int8u some minutes
    //         0x0f: int8u some seconds
    //         0x11: int8u frame number? (0-24 or 0-25)
    let mut pos = g16u(it, &data_pt, n(0))?; // get header length (= 0x1c)
    while pl::lt(&pl::add(&pos, &n(4)), &end) {
        let tag = g16u(it, &data_pt, pos.clone())?;
        if pl::num_eq(&tag, &n(0)) {
            break;
        }
        let mut len = g16u(it, &data_pt, pl::add(&pos, &n(2)))?;
        if pl::num_eq(&tag, &n(0x060e)) {
            len = n(0x10); // (unknown 16 bytes starting with 0x060e2b340253)
        } else {
            pos = pl::add(&pos, &n(4)); // skip tag id/size
            if pl::num_eq(&tag, &n(0x8300)) {
                continue; // descend into contents of 0x8300 (container)
            }
        }
        if pl::gt(&pl::add(&pos, &len), &end) {
            break;
        }
        pl::method(
            it,
            &et,
            "HandleTag",
            vec![tag_table_ptr.clone(), tag, Value::Undef, pl::s("DataPt"), data_pt.clone(), pl::s("DataPos"), data_pos.clone(), pl::s("Start"), pos.clone(), pl::s("Size"), len.clone()],
        )?;
        pos = pl::add(&pos, &len); // step to next tag
    }
    ret(n(1))
}

//------------------------------------------------------------------------------
/// Process SRF maker notes
/// Inputs: 0) ExifTool object ref, 1) dirInfo ref, 2) tag table ref
/// Returns: 1 on success
pub fn process_srf(it: &mut Interp, a: Vec<Value>, want: Want) -> ER<Vec<Value>> {
    let (et, dir_info, mut tag_table_ptr) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    let data_pt = dir_info.get("DataPt");
    let start = dir_info.get("DirStart");
    let verbose = pl::method(it, &et, "Options", vec![pl::s("Verbose")])?;

    // process IFD chain
    let mut ifd = n(0);
    loop {
        // switch tag table for SRF2-5 and SRF6
        if pl::num_eq(&ifd, &n(2)) {
            tag_table_ptr = get_tag_table(it, "Image::ExifTool::Sony::SRF2")?;
        } else if pl::num_eq(&ifd, &n(6)) {
            // SRF6 uses standard EXIF tags
            tag_table_ptr = get_tag_table(it, "Image::ExifTool::Exif::Main")?;
        }
        let srf = pl::concat(&pl::s("SRF"), &ifd);
        dir_info.set("DirName", srf.clone());
        et.set("SET_GROUP1", srf);
        let success = pl::call(it, "Image::ExifTool::Exif::ProcessExif", vec![et.clone(), dir_info.clone(), tag_table_ptr.clone()])?;
        et.delete("SET_GROUP1");
        if !success.truthy() {
            break;
        }
        //
        // get pointer to next IFD
        //
        let count = g16u(it, &data_pt, dir_info.get("DirStart"))?;
        let dir_end = pl::add(&pl::add(&dir_info.get("DirStart"), &n(2)), &pl::mul(&count, &n(12)));
        if pl::gt(&pl::add(&dir_end, &n(4)), &pl::length(&data_pt.deref())) {
            break;
        }
        let mut next_ifd = g32u(it, &data_pt, dir_end)?;
        if !next_ifd.truthy() {
            break;
        }
        next_ifd = pl::sub(&next_ifd, &dir_info.get("DataPos")); // adjust for position of makernotes data
        dir_info.set("DirStart", next_ifd.clone());
        //
        // decrypt next IFD data if necessary
        //
        ifd = pl::add(&ifd, &n(1));
        let (key, len);
        if pl::num_eq(&ifd, &n(1)) {
            // get the key to decrypt IFD1
            let cp = pl::add(&start, &n(0x8ddc)); // why?
            if pl::gt(&pl::add(&cp, &n(1)), &pl::length(&data_pt.deref())) {
                break;
            }
            let t = pl::concat(&pl::concat(&pl::s("x"), &cp), &pl::s(" C"));
            let c = match crate::perl::pack::unpack(&t.to_bytes(), &data_pt.deref().to_bytes()) {
                Ok(v) => v.into_iter().next().unwrap_or_default(),
                Err(e) => return die(e),
            };
            let ip = pl::add(&cp, &pl::mul(&n(4), &c));
            if pl::gt(&pl::add(&ip, &n(4)), &pl::length(&data_pt.deref())) {
                break;
            }
            let t = pl::concat(&pl::concat(&pl::s("x"), &ip), &pl::s(" N"));
            key = match crate::perl::pack::unpack(&t.to_bytes(), &data_pt.deref().to_bytes()) {
                Ok(v) => v.into_iter().next().unwrap_or_default(),
                Err(e) => return die(e),
            };
            len = pl::add(&cp, &next_ifd); // decrypt up to $cp
        } else if pl::num_eq(&ifd, &n(2)) {
            // get the key to decrypt IFD2
            key = et.get("SRF2Key");
            len = pl::sub(&pl::length(&data_pt.deref()), &next_ifd); // decrypt rest of maker notes
        } else {
            continue; // no decryption needed
        }
        // decrypt data
        if !key.is_undef() {
            decrypt_rs(&data_pt, &next_ifd, &len, &key)?;
        }
        if !pl::gt(&verbose, &n(2)) {
            continue;
        }
        // display decrypted data in verbose mode
        pl::method(it, &et, "VerboseDir", vec![pl::concat(&pl::s("Decrypted SRF"), &ifd), n(0), pl::add(&next_ifd, &len)])?;
        let prefix = pl::concat(&et.get("INDENT"), &pl::s("  "));
        pl::method(it, &et, "VerboseDump", vec![data_pt.clone(), pl::s("Prefix"), prefix, pl::s("Start"), next_ifd, pl::s("DataPos"), dir_info.get("DataPos")])?;
    }
    // (the sub ends with the loop: it returns no value)
    pl::ret_nothing(want)
}

//------------------------------------------------------------------------------
/// Write SR2 data
/// Inputs: 0) ExifTool object ref, 1) dirInfo ref, 2) tag table ref
/// Returns: 1 on success when reading, or SR2 directory or undef when writing
pub fn write_sr2(it: &mut Interp, a: Vec<Value>, want: Want) -> ER<Vec<Value>> {
    let (et, dir_info, tag_table_ptr) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    if !et.truthy() {
        return ret(n(1)); // allow dummy access
    }
    let buff = cell(pl::s(""));
    dir_info.set("OutFile", rf(&buff));
    process_sr2(it, vec![et, dir_info, tag_table_ptr], want)
}

//------------------------------------------------------------------------------
/// Read/Write SR2 IFD and its encrypted subdirectories
/// Inputs: 0) ExifTool object ref, 1) dirInfo ref, 2) tag table ref
/// Returns: 1 on success when reading, or SR2 directory or undef when writing
pub fn process_sr2(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info, tag_table_ptr) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    let raf = dir_info.get("RAF");
    let mut data_pt = dir_info.get("DataPt");
    let data_pos = dir_info.get("DataPos");
    let data_len = or(dir_info.get("DataLen"), pl::length(&data_pt.deref()));
    let base = or(dir_info.get("Base"), n(0));
    let outfile = dir_info.get("OutFile");

    // clear SR2 member variables to be safe
    et.delete("SR2SubIFDOffset");
    et.delete("SR2SubIFDLength");
    et.delete("SR2SubIFDKey");

    // make sure we have the first 4 bytes available to test directory type
    let buff = cell(Value::Undef);
    if pl::lt(&data_len, &n(4)) && raf.truthy() {
        let pos = pl::add(&pl::add(&data_pos, &or(dir_info.get("DirStart"), n(0))), &base);
        if pl::method(it, &raf, "Seek", vec![pos.clone(), n(0)])?.truthy() && pl::num_eq(&raf_read_cell(it, &raf, &buff, n(4))?, &n(4)) {
            data_pt = rf(&buff);
            dir_info.set("DataPt", Value::Undef); // must load data from file
            pl::method(it, &raf, "Seek", vec![pos, n(0)])?;
        }
    }
    // this may either be a normal IFD, or a MRW data block
    // (only original ARW images from the A100 use the MRW block)
    if data_pt.truthy() && re(r"^\0MR[IM]", "").is_match(&deref_strict(&data_pt)?) {
        let (mut err, mut srf_pos, mut srf_len) = (Value::Undef, Value::Undef, Value::Undef);
        let data_offset = et.get("A100DataOffset");
        if data_offset.truthy() {
            // save information about the RAW data trailer so it will be preserved
            let kt = Value::new_hash();
            kt.set("Name", pl::s("A100 RAW Data"));
            kt.set("Start", data_offset.clone());
            et.set("KnownTrailer", kt);
        } else {
            err = pl::s("A100DataOffset tag is missing from A100 ARW image");
        }
        if !raf.truthy() {
            err = pl::s("Unrecognized SR2 structure");
        }
        if !err.truthy() {
            srf_pos = pl::method(it, &raf, "Tell", vec![])?;
            srf_len = pl::sub(&data_offset, &srf_pos);
            if !(pl::gt(&srf_len, &n(0)) && pl::num_eq(&raf_read_cell(it, &raf, &buff, srf_len.clone())?, &srf_len)) {
                err = pl::s("Error reading MRW directory");
            }
        }
        if err.truthy() {
            if outfile.truthy() {
                pl::method(it, &et, "Error", vec![err])?;
                return pl::ret_undef();
            }
            pl::method(it, &et, "Warn", vec![err])?;
            return ret(n(0));
        }
        let d = Value::new_hash();
        d.set("DataPt", rf(&buff));
        crate::runtime::require(it, "Image::ExifTool::MinoltaRaw")?;
        if outfile.truthy() {
            // save MRW data to be written last
            let mrw = pl::call(it, "Image::ExifTool::MinoltaRaw::WriteMRW", vec![et.clone(), d])?;
            et.set("MRWDirData", mrw);
            return if et.get("MRWDirData").truthy() { ret(pl::s("\0\0\0\0\0\0")) } else { pl::ret_undef() };
        } else {
            if !outfile.truthy() && et.get("HTML_DUMP").truthy() {
                pl::method(it, &et, "HDump", vec![srf_pos, srf_len, pl::s("[A100 SRF Data]")])?;
            }
            return Ok(vec![pl::call(it, "Image::ExifTool::MinoltaRaw::ProcessMRW", vec![et, d])?]);
        }
    } else if et.get("A100DataOffset").truthy() {
        let err = pl::s("Unexpected A100DataOffset tag");
        if outfile.truthy() {
            pl::method(it, &et, "Error", vec![err])?;
            return pl::ret_undef();
        }
        pl::method(it, &et, "Warn", vec![err])?;
        return ret(n(0));
    }
    let verbose = pl::method(it, &et, "Options", vec![pl::s("Verbose")])?;
    let mut result;
    if outfile.truthy() {
        result = pl::call(it, "Image::ExifTool::Exif::WriteExif", vec![et.clone(), dir_info.clone(), tag_table_ptr.clone()])?;
        if !result.truthy() {
            return pl::ret_undef();
        }
        append(&outfile, &result)?;
    } else {
        result = pl::call(it, "Image::ExifTool::Exif::ProcessExif", vec![et.clone(), dir_info.clone(), tag_table_ptr.clone()])?;
    }
    if !(result.truthy() && et.get("SR2SubIFDOffset").truthy()) {
        return ret(result);
    }
    // only take first offset value if more than one!
    let mut offsets = pl::split_ws(&et.get("SR2SubIFDOffset"));
    let mut offset = if offsets.is_empty() { Value::Undef } else { offsets.remove(0) };
    let length = et.get("SR2SubIFDLength");
    let key = et.get("SR2SubIFDKey");
    let mut subifd_pos: Vec<Value> = Vec::new();
    if offset.truthy() && length.truthy() && !key.is_undef() {
        let buff = cell(Value::Undef);
        // read encrypted SR2SubIFD from file
        let mut got = raf.truthy() && pl::method(it, &raf, "Seek", vec![pl::add(&offset, &base), n(0)])?.truthy() && pl::num_eq(&raf_read_cell(it, &raf, &buff, length.clone())?, &length);
        // or read from data (when processing Adobe DNGPrivateData)
        if !got {
            let rel = pl::sub(&offset, &data_pos);
            if pl::ge(&rel, &n(0)) && pl::lt(&pl::add(&rel, &length), &data_len) {
                let s = substr(it, &deref_strict(&data_pt)?, &rel, Some(&length))?;
                *buff.borrow_mut() = s;
                got = val(&buff).truthy();
            }
        }
        if got {
            decrypt_rs(&rf(&buff), &n(0), &length, &key)?;
            // display decrypted data in verbose mode
            if pl::gt(&verbose, &n(2)) && !outfile.truthy() {
                pl::method(it, &et, "VerboseDir", vec![pl::s("Decrypted SR2SubIFD"), n(0), length.clone()])?;
                pl::method(it, &et, "VerboseDump", vec![rf(&buff), pl::s("Addr"), pl::add(&offset, &base)])?;
            }
            let mut num = pl::s("");
            let d_pos = offset.clone();
            loop {
                let d = Value::new_hash();
                d.set("Base", base.clone());
                d.set("DataPt", rf(&buff));
                d.set("DataLen", pl::length(&val(&buff)));
                d.set("DirStart", pl::sub(&offset, &d_pos));
                d.set("DirName", pl::concat(&pl::s("SR2SubIFD"), &num));
                d.set("DataPos", d_pos.clone());
                let sub_table = get_tag_table(it, "Image::ExifTool::Sony::SR2SubIFD")?;
                if outfile.truthy() {
                    let fixup = pl::method(it, &pl::s("Image::ExifTool::Fixup"), "new", vec![])?;
                    d.set("Fixup", fixup.clone());
                    result = pl::method(it, &et, "WriteDirectory", vec![d, sub_table])?;
                    if !result.truthy() {
                        return pl::ret_undef();
                    }
                    // save position of this SubIFD
                    subifd_pos.push(pl::length(&outfile.deref()));
                    // add this directory to the returned data
                    fixup.set("Start", pl::add(&fixup.get("Start"), &pl::length(&outfile.deref())));
                    append(&outfile, &result)?;
                    pl::method(it, &dir_info.get("Fixup"), "AddFixup", vec![fixup])?;
                } else {
                    result = pl::method(it, &et, "ProcessDirectory", vec![d, sub_table])?;
                }
                if offsets.is_empty() {
                    break;
                }
                offset = offsets.remove(0);
                num = pl::add(&or(num, n(1)), &n(1));
            }
        } else {
            pl::method(it, &et, "Warn", vec![pl::s("Error reading SR2 data")])?;
        }
    }
    if outfile.truthy() && !subifd_pos.is_empty() {
        // the SR2SubIFD must be padded to a multiple of 4 bytes for the encryption
        let mut sr2_len = pl::sub(&pl::length(&outfile.deref()), &subifd_pos[0]);
        if ops::bit_and(&sr2_len, &n(0x03)).truthy() {
            let pad = pl::sub(&n(4), &ops::bit_and(&sr2_len, &n(0x03)));
            sr2_len = pl::add(&sr2_len, &pad);
            append(&outfile, &Value::bytes(vec![b' '; pad.to_i64().max(0) as usize]))?;
        }
        // save the new SR2SubIFD Length and Key to be used later for encryption
        et.set("SR2SubIFDLength", sr2_len.clone());
        let new_key = et.get("VALUE").get("SR2SubIFDKey");
        if !new_key.is_undef() {
            et.set("SR2SubIFDKey", new_key);
        }
        // update SubIFD pointers manually and add to fixup, and set SR2SubIFDLength
        let nn = g16u(it, &outfile, n(0))?;
        let found = Value::new_hash();
        let mut i = n(0);
        while pl::lt(&i, &nn) {
            let entry = pl::add(&n(2), &pl::mul(&n(12), &i));
            i = pl::add(&i, &n(1));
            let tag_id = g16u(it, &outfile, entry.clone())?;
            // only interested in SR2SubIFDOffset (0x7200) and SR2SubIFDLength (0x7201)
            if !(pl::num_eq(&tag_id, &n(0x7200)) || pl::num_eq(&tag_id, &n(0x7201))) {
                continue;
            }
            found.set(tag_id.to_bytes(), n(1));
            let fmt = g16u(it, &outfile, pl::add(&entry, &n(2)))?;
            if !pl::num_eq(&fmt, &n(0x04)) {
                // must be int32u
                pl::method(it, &et, "Error", vec![pl::concat(&pl::concat(&pl::s("Unexpected format ("), &fmt), &pl::s(") for SR2SubIFD tag"))])?;
                return pl::ret_undef();
            }
            if pl::num_eq(&tag_id, &n(0x7201)) {
                // SR2SubIFDLength
                s32u_at(it, sr2_len.clone(), &outfile, pl::add(&entry, &n(8)))?;
                continue;
            }
            let mut tag = pl::s("SR2SubIFDOffset");
            let mut value_ptr = if subifd_pos.len() < 2 { pl::add(&entry, &n(8)) } else { g32u(it, &outfile, pl::add(&entry, &n(8)))? };
            for pos in &subifd_pos {
                s32u_at(it, pos.clone(), &outfile, value_ptr.clone())?;
                pl::method(it, &dir_info.get("Fixup"), "AddFixup", vec![value_ptr.clone(), tag])?;
                tag = Value::Undef;
                value_ptr = pl::add(&value_ptr, &n(4));
            }
        }
        if !(found.get(n(0x7200).to_bytes()).truthy() && found.get(n(0x7201).to_bytes()).truthy()) {
            pl::method(it, &et, "Error", vec![pl::s("Missing SR2SubIFD tag")])?;
            return pl::ret_undef();
        }
    }
    ret(if outfile.truthy() { outfile.deref() } else { result })
}

/// Register the subs ported so far; the others still run from the Perl.
pub fn register(it: &mut Interp) {
    it.define_native("Image::ExifTool::Sony::SortLensTypes", sort_lens_types);
    it.define_native("Image::ExifTool::Sony::ReadHiddenData", read_hidden_data);
    it.define_native("Image::ExifTool::Sony::ProcessSonyPIC", process_sony_pic);
    it.define_native("Image::ExifTool::Sony::ConvMeter1", conv_meter1);
    it.define_native("Image::ExifTool::Sony::ConvMeter2", conv_meter2);
    it.define_native("Image::ExifTool::Sony::ConvLensSpec", conv_lens_spec);
    it.define_native("Image::ExifTool::Sony::ConvInvLensSpec", conv_inv_lens_spec);
    it.define_native("Image::ExifTool::Sony::PrintLensSpec", print_lens_spec);
    it.define_native("Image::ExifTool::Sony::PrintInvLensSpec", print_inv_lens_spec);
    it.define_native("Image::ExifTool::Sony::ProcessMoreInfo", process_more_info);
    it.define_native("Image::ExifTool::Sony::ProcessPMP", process_pmp);
    it.define_native("Image::ExifTool::Sony::SetARW", set_arw);
    it.define_native("Image::ExifTool::Sony::FinishARW", finish_arw);
    it.define_native("Image::ExifTool::Sony::Decrypt", decrypt);
    it.define_native("Image::ExifTool::Sony::Decipher", decipher);
    it.define_native("Image::ExifTool::Sony::ProcessEnciphered", process_enciphered);
    it.define_native("Image::ExifTool::Sony::WriteEnciphered", write_enciphered);
    it.define_native("Image::ExifTool::Sony::Process_rtmd", process_rtmd);
    it.define_native("Image::ExifTool::Sony::ProcessSRF", process_srf);
    it.define_native("Image::ExifTool::Sony::WriteSR2", write_sr2);
    it.define_native("Image::ExifTool::Sony::ProcessSR2", process_sr2);
}
