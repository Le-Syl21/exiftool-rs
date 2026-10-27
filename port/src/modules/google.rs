//! Image::ExifTool::Google (Image/ExifTool/Google.pm), ported sub by sub (see docs/port/PORTING.md).
//!
//! Google maker notes (HDRP) and XMP tags. The tag tables are data; these
//! are the module's two subs.

use crate::interp::eval::ER;
use crate::interp::{Interp, Want};
use crate::modules::quick_time_stream::{get_tag_table, len_of, m, match_from, n, opt, s, warn};
use crate::perl::{ops, Value};
use crate::pl::{self, arg, cap, re, ret};
use crate::runtime;

/// Read HDRP text-format maker note version 2
/// Inputs: 0) ExifTool ref, 1) data ref
pub fn process_hdrp_maker_note(it: &mut Interp, a: Vec<Value>, want: Want) -> ER<Vec<Value>> {
    let (et, data_pt) = (arg(&a, 0), arg(&a, 1));
    let data = data_pt.deref();
    let mut tag = Value::Undef;
    let mut pos = Value::Undef;
    let tag_tbl = get_tag_table(it, "Image::ExifTool::Google::HDRPMakerNote")?;
    let mut p = 0usize; // pos($$dataPt)
    let mut c1 = Value::Undef;
    loop {
        let (end, last);
        if let Some((c, e)) = match_from(r"^ ?([A-Z].*)$", "m", &data, p) {
            p = e;
            c1 = cap(&c, 1);
            end = n(p as i64 - len_of(&c1));
            last = false;
        } else {
            p = 0;
            end = n(len_of(&data));
            last = true;
        }
        if tag.truthy() {
            let len = pl::sub(&end, &pl::add(&pos, &n(1)));
            if pl::le(&len, &n(0)) {
                break; // (just to be safe)
            }
            let v = pl::substr(&data, pl::add(&pos, &n(1)).to_i64(), Some(len.to_i64()));
            m(it, &et, "HandleTag", vec![tag_tbl.clone(), tag.clone(), v, s("MakeTagInfo"), n(1)])?;
        }
        if last {
            break;
        }
        tag = c1.clone();
        if !(re(r":", "").is_match(&tag) || re(r"^\w+$", "").is_match(&tag)) {
            m(it, &et, "HandleTag", vec![tag_tbl.clone(), s("ProcessingNotes"), tag.clone()])?;
            tag = Value::Undef;
            continue;
        }
        pos = n(p as i64);
        let mut c_1 = Value::Undef;
        let mut c_2 = Value::Undef;
        let did = re(r"( \(base64\))?: ?(.*)", "").subst(&mut tag, false, |c| {
            c_1 = cap(c, 1);
            c_2 = cap(c, 2);
            Vec::new()
        }) > 0;
        if did && c_2.truthy() {
            let mut dat = c_2.clone();
            if c_1.truthy() {
                dat = pl::call(it, "Image::ExifTool::XMP::DecodeBase64", vec![dat])?;
            }
            m(it, &et, "HandleTag", vec![tag_tbl.clone(), tag.clone(), dat, s("MakeTagInfo"), n(1)])?;
            tag = Value::Undef;
        }
    }
    let _ = p;
    pl::ret_nothing(want)
}

/// Decode HDRP maker notes (ref https://github.com/jakiki6/ruminant/blob/master/ruminant/modules/images.py)
/// Inputs: 0) ExifTool ref, 1) base64-encoded string, 2) tagInfo ref
/// Returns: reference to decoded+decrypted+gunzipped data
/// - also extracts protobuf info as separate tags when Unknown used if $$tagInfo{IsProtobuf} is set
pub fn process_hdrp(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info, tag_tbl) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    let mut data_pt = dir_info.get("DataPt");
    let tag_info = dir_info.get("TagInfo");
    let tag_name = if tag_info.truthy() { tag_info.get("Name") } else { s("") };
    let verbose = opt(it, &et, "Verbose")?;
    let fs = opt(it, &et, "FastScan")?;
    let fast = if fs.truthy() { fs } else { n(0) };
    let ver;
    let val_pt;

    if pl::gt(&fast, &n(1)) {
        return pl::ret_undef();
    }

    if dir_info.get("DirStart").truthy() {
        let dat = pl::substr(&data_pt.deref(), dir_info.get("DirStart").to_i64(), Some(dir_info.get("DirLen").to_i64()));
        data_pt = pl::sref(dat);
    }
    if re(r"^HDRP[\x02\x03]", "").is_match(&data_pt.deref()) {
        val_pt = data_pt.clone();
    } else {
        m(it, &et, "VerboseDir", vec![tag_name.clone(), Value::Undef, n(len_of(&data_pt.deref()))])?;
        if pl::gt(&verbose, &n(2)) {
            m(it, &et, "VerboseDump", vec![data_pt.clone()])?;
        }
        val_pt = pl::call(it, "Image::ExifTool::XMP::DecodeBase64", vec![data_pt.deref()])?;
        if pl::gt(&verbose, &n(2)) {
            m(it, &et, "VerboseDir", vec![pl::concat(&s("Base64-decoded "), &tag_name), Value::Undef, n(len_of(&val_pt.deref()))])?;
            m(it, &et, "VerboseDump", vec![val_pt.clone()])?;
        }
    }
    let mut vv = val_pt.deref();
    let mut c1 = Value::Undef;
    if re(r"^HDRP([\x02\x03])", "").subst(&mut vv, false, |c| {
        c1 = cap(c, 1);
        Vec::new()
    }) > 0
    {
        pl::set_deref(&val_pt, vv.clone())?;
        ver = n(c1.to_bytes()[0] as i64);
    } else {
        warn(it, &et, s("Unrecognized HDRP format"))?;
        return pl::ret_undef();
    }
    let pad = (8 - (len_of(&vv) % 8)) & 0x07;
    if pad != 0 {
        vv = pl::concat(&vv, &Value::bytes(vec![0u8; pad as usize])); // pad to an even 8 bytes
        pl::set_deref(&val_pt, vv.clone())?;
    }
    let mut words = pl::unpack("V*", &vv)?;
    // my $key = 0x2515606b4a7791cd;
    let (mut hi, mut lo) = (n(0x2515606b), n(0x4a7791cd));
    let mut i = 0usize;
    let (and, or, xor, shr, shl) = (ops::bit_and, ops::bit_or, ops::bit_xor, ops::shift_right, ops::shift_left);
    while i < words.len() {
        // (messy, but handle all 64-bit arithmetic with 32-bit backward
        //  compatibility, so no bit operations on any number > 0xffffffff)
        // rotate the key for each new 64-bit word
        // $key ^= $key >> 12;
        lo = xor(&lo, &or(&shr(&lo, &n(12)), &shl(&and(&hi, &n(0xfff)), &n(20))));
        hi = xor(&hi, &shr(&hi, &n(12)));
        // $key ^= ($key << 25) & 0xffffffffffffffff;
        hi = xor(&hi, &or(&shl(&and(&hi, &n(0x7f)), &n(25)), &shr(&lo, &n(7))));
        lo = xor(&lo, &shl(&and(&lo, &n(0x7f)), &n(25)));
        // $key ^= ($key >> 27) & 0xffffffffffffffff;
        lo = xor(&lo, &or(&shr(&lo, &n(27)), &shl(&and(&hi, &n(0x7ffffff)), &n(5))));
        hi = xor(&hi, &shr(&hi, &n(27)));
        // $key = ($key * 0x2545f4914f6cdd1d) & 0xffffffffffffffff;
        // (multiply using 16-bit math to avoid overflowing 32-bit integers)
        let av = pl::unpack("n*", &pl::pack("N*", &[hi.clone(), lo.clone()])?)?;
        let b = [n(0x2545), n(0xf491), n(0x4f6c), n(0xdd1d)];
        let mut c = vec![n(0); 7];
        for j in 0..4 {
            for k in 0..4 {
                c[j + k] = pl::add(&c[j + k], &pl::mul(&av[j], &b[k]));
            }
        }
        // (we will only retain the low 64-bits of the key, so
        //  don't bother finishing the calculation of the upper bits)
        for j in (3..=6).rev() {
            while pl::gt(&c[j], &n(0xffffffff)) {
                c[j - 2] = pl::add(&c[j - 2], &n(1));
                c[j] = pl::sub(&c[j], &n(4294967296));
            }
            c[j - 1] = pl::add(&c[j - 1], &shr(&c[j], &n(16)));
            c[j] = and(&c[j], &n(0xffff));
        }
        hi = pl::add(&shl(&c[3], &n(16)), &c[4]);
        lo = pl::add(&shl(&c[5], &n(16)), &c[6]);
        // apply the key to this 64-bit word
        if i >= words.len() {
            words.resize(i + 1, Value::Undef);
        }
        words[i] = xor(&words[i], &lo);
        i += 1;
        if i >= words.len() {
            words.resize(i + 1, Value::Undef);
        }
        words[i] = xor(&words[i], &hi);
        i += 1;
    }
    let mut result = Value::Undef;
    let mut val = pl::pack("V*", &words)?;
    if pad != 0 {
        val = pl::substr(&val, 0, Some(-pad)); // remove padding
    }
    if pl::gt(&verbose, &n(2)) {
        m(it, &et, "VerboseDir", vec![pl::concat(&s("Decrypted "), &tag_name), Value::Undef, n(len_of(&val))])?;
        m(it, &et, "VerboseDump", vec![pl::sref(val.clone())])?;
    }
    if pl::eval_block(it, |it| runtime::require(it, "IO::Uncompress::Gunzip"))?.is_some() {
        let buff = pl::sref(Value::Undef);
        if pl::call(it, "IO::Uncompress::Gunzip::gunzip", vec![pl::sref(val.clone()), buff.clone()])?.truthy() {
            if pl::gt(&verbose, &n(2)) {
                m(it, &et, "VerboseDir", vec![pl::concat(&s("Gunzipped "), &tag_name), Value::Undef, n(len_of(&buff.deref()))])?;
                m(it, &et, "VerboseDump", vec![buff.clone()])?;
            }
            if pl::num_eq(&ver, &n(3)) || (tag_info.truthy() && tag_info.get("IsProtobuf").truthy()) {
                let di = Value::new_hash();
                di.set("DataPt", buff.clone());
                di.set("DirName", tag_name.clone());
                runtime::require(it, "Image::ExifTool::Protobuf")?;
                pl::call(it, "Image::ExifTool::Protobuf::ProcessProtobuf", vec![et.clone(), di, tag_tbl.clone()])?;
            } else {
                process_hdrp_maker_note(it, vec![et.clone(), buff.clone()], Want::Void)?;
            }
            result = buff;
        } else {
            let e = it.get_scalar("IO::Uncompress::Gunzip::GunzipError");
            warn(it, &et, pl::concat(&s("Error inflating stream: "), &e))?;
        }
    } else {
        warn(it, &et, s("Install IO::Uncompress::Gunzip to decode HDRP makernote"))?;
    }
    ret(result)
}

pub fn register(it: &mut Interp) {
    it.define_native("Image::ExifTool::Google::ProcessHDRPMakerNote", process_hdrp_maker_note);
    it.define_native("Image::ExifTool::Google::ProcessHDRP", process_hdrp);
}
