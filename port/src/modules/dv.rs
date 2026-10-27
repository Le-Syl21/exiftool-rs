//! Image::ExifTool::DV (Image/ExifTool/DV.pm), ported sub by sub (see docs/port/PORTING.md).
//!
//! Read DV meta information

use crate::interp::eval::ER;
use crate::interp::{Interp, Want};
use crate::modules::random_access::{cell, raf_read_cell, substr};
use crate::perl::Value;
use crate::pl::{self, arg, n, ret};

const MODULE: &str = "Image::ExifTool::DV";

/// `Image::ExifTool::NAME(ARGS)` (the functions imported with :DataAccess :Utils).
fn call(it: &mut Interp, name: &str, args: Vec<Value>) -> ER<Value> {
    pl::call(it, &format!("Image::ExifTool::{name}"), args)
}

/// The spans of every match of `pat` in `subject` (`while ($s =~ /re/g)`).
fn match_spans(pat: &[u8], subject: &[u8]) -> ER<Vec<(usize, usize)>> {
    let rx = match crate::perl::regex::compile(pat, "s") {
        Ok(r) => r,
        Err(e) => return crate::interp::eval::die(e),
    };
    Ok(rx.match_all(subject, 0).unwrap_or_default().iter().filter_map(|sp| sp[0]).collect())
}

/// Read information in a DV file (ref 1)
/// Inputs: 0) ExifTool ref, 1) dirInfo ref
/// Returns: 1 on success, 0 if this wasn't a valid DV file
pub fn process_dv(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    pl::local_topic(it, |it| process_dv_body(it, &a))
}

fn process_dv_body(it: &mut Interp, a: &[Value]) -> ER<Vec<Value>> {
    let (et, dir_info) = (arg(a, 0), arg(a, 1));
    let raf = dir_info.get("RAF");
    let buff = cell(Value::Undef);
    let buff_ref = Value::ScalarRef(buff.clone());
    let mut start: Option<i64> = None;
    let mut profile = Value::Undef;
    let get8 = |it: &mut Interp, p: i64| -> ER<i64> { Ok(call(it, "Get8u", vec![buff_ref.clone(), n(p)])?.to_i64()) };

    if !raf_read_cell(it, &raf, &buff, n(12000))?.truthy() {
        return ret(n(0));
    }
    let b = buff.borrow().clone();
    let bytes = b.to_bytes();
    if let Some(&(_, e)) = match_spans(br"\x1f\x07\0[\x3f\xbf]", &bytes)?.first() {
        start = Some(e as i64 - 4);
    } else {
        for (_, e) in match_spans(br"[\0\xff]\x3f\x07\0.{76}\xff\x3f\x07\x01", &bytes)? {
            if (e as i64) - 163 < 0 {
                continue;
            }
            start = Some(e as i64 - 163);
            break;
        }
        if start.is_none() {
            return ret(n(0));
        }
    }
    let start = start.unwrap();
    let len = bytes.len() as i64;
    // must at least have a full DIF header
    if start + 80 * 6 > len {
        return ret(n(0));
    }

    pl::method(it, &et, "SetFileType", vec![])?;

    let mut pos = start;
    let dsf = (get8(it, pos + 3)? & 0x80) >> 7;
    let stype = get8(it, pos + 80 * 5 + 48 + 3)? & 0x1f;
    let dv_profiles = crate::runtime::lex_array(it, MODULE, "dvProfiles");

    // 576i50 25Mbps 4:1:1 is a special case
    if dsf == 1 && stype == 0 && get8(it, 4)? & 0x07 != 0 {
        profile = dv_profiles.at(2);
    } else {
        for p in dv_profiles.items() {
            if !(pl::num_eq(&n(dsf), &p.get("DSF")) && pl::num_eq(&n(stype), &p.get("VideoSType"))) {
                continue;
            }
            profile = p;
            break;
        }
        if !profile.truthy() {
            pl::method(it, &et, "Warn", vec![pl::s("Unrecognized DV profile")])?;
            return ret(n(1));
        }
    }
    let tag_table_ptr = call(it, "GetTagTable", vec![pl::s("Image::ExifTool::DV::Main")])?;

    // calculate total bit rate and duration
    let byte_rate = pl::mul(&profile.get("FrameSize"), &profile.get("FrameRate"));
    let file_size = et.get("VALUE").get("FileSize");
    profile.set("TotalBitrate", pl::mul(&n(8), &byte_rate));
    if !file_size.is_undef() {
        profile.set("Duration", pl::div(&file_size, &byte_rate)?);
    }

    // read DVPack metadata from the VAUX DIF's to extract video tags
    profile.delete("DateTimeOriginal");
    profile.delete("AspectRatio");
    profile.delete("VideoScanType");
    let (mut date, mut time, mut is16_9, mut interlace) = (Value::Undef, Value::Undef, Value::Undef, Value::Undef);
    for _i in 1..6 {
        pos += 80;
        let type_ = get8(it, pos)?;
        if (type_ & 0xf0) != 0x50 {
            continue; // look for VAUX types
        }
        for j in 0..15 {
            let p = pos + j * 5 + 3;
            let type_ = get8(it, p)?;
            if type_ == 0x61 {
                // video control
                let apt = get8(it, start + 4)? & 0x07;
                let t = get8(it, p + 2)?;
                is16_9 = Value::from((t & 0x07) == 0x02 || (apt == 0 && (t & 0x07) == 0x07));
                interlace = n(get8(it, p + 3)? & 0x10); // (ref 2)
            } else if type_ == 0x62 {
                // date
                // mask off unused bits
                let s = substr(it, &b, &n(p + 1), Some(&n(4)))?;
                let d = pl::unpack("C*", &s)?;
                // (ignore timezone in byte 0 until we can test this properly - see ref 2)
                date = pl::sprintf("%.2x:%.2x:%.2x", &[arg(&d, 3), n(arg(&d, 2).to_i64() & 0x1f), n(arg(&d, 1).to_i64() & 0x3f)]);
                if pl::re("[a-f]", "").is_match(&date) {
                    date = Value::Undef; // invalid date
                } else {
                    // add century (this will work until 2089)
                    let century = if date.to_bytes()[..] < b"9"[..] { "20" } else { "19" };
                    date = pl::concat(&pl::s(century), &date);
                }
                time = Value::Undef;
            } else if type_ == 0x63 && date.truthy() {
                // time
                // (ignore frames past second in byte 0 for now - see ref 2)
                let _val = call(it, "Get32u", vec![buff_ref.clone(), n(p + 1)])?.to_i64() & 0x007f7f3f;
                let s = substr(it, &b, &n(p + 1), Some(&n(4)))?;
                let t = pl::unpack("C*", &s)?;
                time = pl::sprintf("%.2x:%.2x:%.2x", &[n(arg(&t, 3).to_i64() & 0x3f), n(arg(&t, 2).to_i64() & 0x7f), n(arg(&t, 1).to_i64() & 0x7f)]);
                break;
            } else {
                time = Value::Undef; // must be consecutive
            }
        }
    }
    if date.truthy() && time.truthy() {
        profile.set("DateTimeOriginal", pl::concat(&pl::concat(&date, &pl::s(" ")), &time));
        if !is16_9.is_undef() {
            profile.set("AspectRatio", pl::s(if is16_9.truthy() { "16:9" } else { "4:3" }));
            profile.set("VideoScanType", pl::s(if interlace.truthy() { "Interlaced" } else { "Progressive" }));
        }
    }

    // read audio tags if available
    profile.delete("AudioSampleRate");
    profile.delete("AudioBitsPerSample");
    profile.delete("AudioChannels");
    pos = start + 80 * 6 + 80 * 16 * 3 + 3;
    if pos + 4 < len && get8(it, pos)? == 0x50 {
        let _smpls = get8(it, pos + 1)?;
        let freq = (get8(it, pos + 4)? >> 3) & 0x07;
        let mut stype = get8(it, pos + 3)? & 0x1f;
        let quant = get8(it, pos + 4)? & 0x07;
        if freq < 3 {
            profile.set("AudioSampleRate", n([48000, 44100, 32000][freq as usize]));
        }
        if stype < 3 {
            if stype == 0 && quant != 0 && freq == 2 {
                stype = 2;
            }
            profile.set("AudioChannels", n([2, 0, 4, 8][stype as usize]));
        }
        profile.set("AudioBitsPerSample", n(if quant != 0 { 12 } else { 16 }));
    }

    // save our metadata
    let dv_tags = crate::runtime::lex_array(it, MODULE, "dvTags");
    for tag in dv_tags.items() {
        let v = profile.get(tag.to_bytes());
        if v.is_undef() {
            continue;
        }
        pl::method(it, &et, "HandleTag", vec![tag_table_ptr.clone(), tag, v])?;
    }

    ret(n(1))
}

pub fn register(it: &mut Interp) {
    it.define_native("Image::ExifTool::DV::ProcessDV", process_dv);
}
