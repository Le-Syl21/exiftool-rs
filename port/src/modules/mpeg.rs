//! Image::ExifTool::MPEG (Image/ExifTool/MPEG.pm), ported sub by sub (see docs/port/PORTING.md).
//!
//! Read MPEG-1 and MPEG-2 meta information
//!
//! The frame-sync searches are `m//g` matches whose pos() lives on the
//! caller's scalar (`$$buffPt`): they run as Perl statements through
//! `perl_eval`, so pos() is read and written on that same scalar.

use crate::interp::eval::ER;
use crate::interp::{Interp, Want};
use crate::modules::random_access::{cell, index, perl_eval, raf_read_cell};
use crate::perl::Value;
use crate::pl::{self, arg, cap, n, re, ret};

const MODULE: &str = "Image::ExifTool::MPEG";

/// `Image::ExifTool::NAME(ARGS)` (the functions imported with :DataAccess :Utils).
fn call(it: &mut Interp, name: &str, args: Vec<Value>) -> ER<Value> {
    pl::call(it, &format!("Image::ExifTool::{name}"), args)
}

fn m(it: &mut Interp, et: &Value, name: &str, args: Vec<Value>) -> ER<Value> {
    pl::method(it, et, name, args)
}

/// A Perl statement on `$buffPt` (the reference to the caller's buffer), in list context.
fn on_buff(it: &mut Interp, code: &str, buff_pt: &Value) -> ER<Vec<Value>> {
    let c = cell(buff_pt.clone());
    perl_eval(it, code, MODULE, &[("buffPt", &c)], Want::List)
}

/// `pos($$buffPt)`
fn pos_of(it: &mut Interp, buff_pt: &Value) -> ER<Value> {
    Ok(on_buff(it, "pos($$buffPt)", buff_pt)?.pop().unwrap_or_default())
}

/// `pos($$buffPt) = $p`
fn set_pos(it: &mut Interp, buff_pt: &Value, p: &Value) -> ER<()> {
    let (c, pc) = (cell(buff_pt.clone()), cell(p.clone()));
    perl_eval(it, "pos($$buffPt) = $p", MODULE, &[("buffPt", &c), ("p", &pc)], Want::Void)?;
    Ok(())
}

/// The first value of a sub's result list in scalar context.
fn first(r: Vec<Value>) -> Value {
    r.into_iter().last().unwrap_or_default()
}

/// Process information in an MPEG audio or video frame header
/// Inputs: 0) ExifTool object ref, 1) tag table ref, 2-N) list of 32-bit data words
pub fn process_frame_header(it: &mut Interp, a: Vec<Value>, want: Want) -> ER<Vec<Value>> {
    let (et, tag_table_ptr) = (arg(&a, 0), arg(&a, 1));
    let data: Vec<Value> = a.iter().skip(2).cloned().collect();
    let mut keys: Vec<Value> = tag_table_ptr.keys().into_iter().map(Value::from).collect();
    keys.sort_by(|x, y| x.to_bytes().cmp(&y.to_bytes()));
    for tag in keys {
        let Some(c) = re(r"^Bit(\d{2})-?(\d{2})?", "").m(&tag) else { continue };
        let b1 = cap(&c, 1);
        let b2 = if cap(&c, 2).truthy() { cap(&c, 2) } else { b1.clone() };
        let index = pl::int(&pl::div(&b1, &n(32))?);
        let word = arg(&data, index.to_i64().max(0) as usize);
        let mut mask: u64 = 0;
        let span = pl::sub(&b2, &b1).to_i64();
        for i in 0..=span {
            mask = mask.wrapping_add(1u64.wrapping_shl(i as u32));
        }
        let shift = pl::sub(&pl::add(&n(31), &pl::mul(&n(32), &index)), &b2).to_i64();
        let w = word.to_i64() as u64;
        let v = if (0..64).contains(&shift) { w >> shift } else { 0 } & mask;
        m(it, &et, "HandleTag", vec![tag_table_ptr.clone(), tag, Value::Int(v as i64)])?;
    }
    // (the sub has no return: the foreach gives an empty list)
    pl::ret_nothing(want)
}

/// Read MPEG audio frame header
/// Inputs: 0) ExifTool object reference, 1) Reference to audio data
///         2) flag set if we are trying to recognized MP3 file only
/// Returns: 1 on success, 0 if no audio header was found
pub fn parse_mpeg_audio(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, buff_pt, mp3) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    let word;
    let mut pos;
    let ext = {
        let e = et.get("FILE_EXT");
        if e.truthy() { e } else { pl::s("") }
    };

    loop {
        // find frame sync
        let r = on_buff(it, r"$$buffPt =~ m{(\xff.{3})}sg ? ($1) : ()", &buff_pt)?;
        let Some(w) = r.into_iter().next() else { return ret(n(0)) };
        let wd = pl::unpack1("N", &w)?; // get audio frame header word
        let wv = wd.to_i64();
        if (wv & 0xffe00000) != 0xffe00000 {
            let p = pos_of(it, &buff_pt)?;
            set_pos(it, &buff_pt, &pl::sub(&p, &n(2)))?; // next possible location for frame sync
            continue;
        }
        // validate header as much as possible
        if (wv & 0x180000) == 0x080000 ||   // 01 is a reserved version ID
            (wv & 0x060000) == 0x000000 ||  // 00 is a reserved layer description
            (wv & 0x00f000) == 0x000000 ||  // 0000 is the "free" bitrate index
            (wv & 0x00f000) == 0x00f000 ||  // 1111 is a bad bitrate index
            (wv & 0x000c00) == 0x000c00 ||  // 11 is a reserved sampling frequency
            (wv & 0x000003) == 0x000002 ||  // 10 is a reserved emphasis
            (mp3.truthy() && (wv & 0x060000) != 0x020000)
        // must be layer 3 for MP3
        {
            // give up easily unless this really should be an MP3 file
            if !pl::str_eq(&ext, "MP3") {
                return ret(n(0));
            }
            let p = pos_of(it, &buff_pt)?;
            set_pos(it, &buff_pt, &pl::sub(&p, &n(1)))?;
            continue;
        }
        word = wd;
        pos = pos_of(it, &buff_pt)?;
        break;
    }
    // set file type if not done already
    m(it, &et, "SetFileType", vec![])?;

    let tag_table_ptr = call(it, "GetTagTable", vec![pl::s("Image::ExifTool::MPEG::Audio")])?;
    process_frame_header(it, vec![et.clone(), tag_table_ptr, word], Want::Void)?;

    // extract the VBR information (ref MP3::Info)
    let (v, mm) = (et.get("MPEG_Vers"), et.get("MPEG_Mode"));
    #[allow(clippy::never_loop)]
    while !v.is_undef() && !mm.is_undef() {
        let data = buff_pt.deref();
        let len = pl::length(&data);
        let inc = if pl::num_eq(&v, &n(3)) {
            if pl::num_eq(&mm, &n(3)) { 17 } else { 32 }
        } else if pl::num_eq(&mm, &n(3)) {
            9
        } else {
            17
        };
        pos = pl::add(&pos, &n(inc));
        if pl::gt(&pl::add(&pos, &n(8)), &len) {
            break;
        }
        let buff = pl::substr(&data, pos.to_i64(), Some(8));
        if !re(r"^(Xing|Info)", "").is_match(&buff) {
            break;
        }
        let xing_table = call(it, "GetTagTable", vec![pl::s("Image::ExifTool::MPEG::Xing")])?;
        let mut vbr_scale = Value::Undef;
        let flags = pl::unpack1("x4N", &buff)?.to_i64();
        let is_vbr = !re(r"^Info", "").is_match(&buff); // Info frame is not VBR (ref 5)
        pos = pl::add(&pos, &n(8));
        let at = |pos: &Value| format!("x{}N", pos.to_text());
        if flags & 0x01 != 0 {
            // VBRFrames
            if pl::gt(&pl::add(&pos, &n(4)), &len) {
                break;
            }
            if is_vbr {
                let mut args = vec![xing_table.clone(), n(1)];
                args.extend(pl::unpack(&at(&pos), &data)?);
                m(it, &et, "HandleTag", args)?;
            }
            pos = pl::add(&pos, &n(4));
        }
        if flags & 0x02 != 0 {
            // VBRBytes
            if pl::gt(&pl::add(&pos, &n(4)), &len) {
                break;
            }
            if is_vbr {
                let mut args = vec![xing_table.clone(), n(2)];
                args.extend(pl::unpack(&at(&pos), &data)?);
                m(it, &et, "HandleTag", args)?;
            }
            pos = pl::add(&pos, &n(4));
        }
        if flags & 0x04 != 0 {
            // VBR_TOC
            if pl::gt(&pl::add(&pos, &n(100)), &len) {
                break;
            }
            // (ignore toc for now)
            pos = pl::add(&pos, &n(100));
        }
        if flags & 0x08 != 0 {
            // VBRScale
            if pl::gt(&pl::add(&pos, &n(4)), &len) {
                break;
            }
            vbr_scale = pl::unpack1(&at(&pos), &data)?;
            if is_vbr {
                m(it, &et, "HandleTag", vec![xing_table.clone(), n(3), vbr_scale.clone()])?;
            }
            pos = pl::add(&pos, &n(4));
        }
        // process Lame header (ref 5)
        if flags & 0x10 != 0 {
            // Lame
            if pl::gt(&pl::add(&pos, &n(348)), &len) {
                break;
            }
        } else if pl::le(&pl::add(&pos, &n(4)), &len) {
            let mut lib = pl::substr(&data, pos.to_i64(), Some(4));
            if !(pl::str_eq(&lib, "LAME") || pl::str_eq(&lib, "GOGO")) {
                // attempt to identify other encoders
                let mut nn;
                if index(&data, &pl::s("RCA mp3PRO Encoder"), None).to_i64() >= 0 {
                    lib = pl::s("RCA mp3PRO");
                } else if {
                    nn = index(&data, &pl::s("THOMSON mp3PRO Encoder"), None);
                    nn.to_i64() >= 0
                } {
                    lib = pl::s("Thomson mp3PRO");
                    nn = pl::add(&nn, &n(22));
                    if pl::ge(&pl::sub(&pl::length(&data), &nn), &n(6)) {
                        lib = pl::concat(&lib, &pl::concat(&pl::s(" "), &pl::substr(&data, nn.to_i64(), Some(6))));
                    }
                } else if index(&data, &pl::s("MPGE"), None).to_i64() >= 0 {
                    lib = pl::s("Gogo (<3.0)");
                } else {
                    break;
                }
                m(it, &et, "HandleTag", vec![xing_table.clone(), n(4), lib])?;
                break;
            }
        }
        let lame_len = pl::sub(&len, &pos);
        if pl::lt(&lame_len, &n(9)) {
            break;
        }
        let enc = pl::substr(&data, pos.to_i64(), Some(9));
        if enc.to_bytes()[..] >= b"LAME3.90"[..] {
            m(it, &et, "HandleTag", vec![xing_table.clone(), n(4), enc])?;
            if pl::le(&vbr_scale, &n(100)) {
                let d = pl::sub(&n(100), &vbr_scale);
                m(it, &et, "HandleTag", vec![xing_table.clone(), n(5), pl::int(&pl::div(&d, &n(10))?)])?;
                m(it, &et, "HandleTag", vec![xing_table.clone(), n(6), pl::modulo(&d, &n(10))?])?;
            }
            let dir_info = Value::new_hash();
            dir_info.set("DataPt", buff_pt.clone());
            dir_info.set("DirStart", pos.clone());
            dir_info.set("DirLen", pl::sub(&pl::length(&buff_pt.deref()), &pos));
            let sub_table_ptr = call(it, "GetTagTable", vec![pl::s("Image::ExifTool::MPEG::Lame")])?;
            m(it, &et, "ProcessDirectory", vec![dir_info, sub_table_ptr])?;
        } else {
            m(it, &et, "HandleTag", vec![xing_table.clone(), n(4), pl::substr(&data, pos.to_i64(), Some(20))])?;
        }
        break; // (didn't want to loop anyway)
    }

    ret(n(1))
}

/// Read MPEG video frame header
/// Inputs: 0) ExifTool object reference, 1) Reference to video data
/// Returns: 1 on success, 0 if no video header was found
pub fn process_mpeg_video(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, buff_pt) = (arg(&a, 0), arg(&a, 1));

    if !pl::ge(&pl::length(&buff_pt.deref()), &n(4)) {
        return ret(n(0));
    }
    let u = pl::unpack("N2", &buff_pt.deref())?;
    let (w1, w2) = (arg(&u, 0), arg(&u, 1));
    let w = w1.to_i64();
    // validate as much as possible
    if (w & 0x000000f0) == 0x00000000 ||   // 0000 is a forbidden aspect ratio
        (w & 0x000000f0) == 0x000000f0 ||  // 1111 is a reserved aspect ratio
        (w & 0x0000000f) == 0 ||           // frame rate must be 1-8
        (w & 0x0000000f) > 8
    {
        return ret(n(0));
    }
    // set file type if not done already
    if !et.get("FileType").truthy() {
        m(it, &et, "SetFileType", vec![pl::s("MPEG")])?;
    }

    let tag_table_ptr = call(it, "GetTagTable", vec![pl::s("Image::ExifTool::MPEG::Video")])?;
    process_frame_header(it, vec![et.clone(), tag_table_ptr, w1, w2], Want::Void)?;
    ret(n(1))
}

/// Read MPEG audio and video frame headers
/// Inputs: 0) ExifTool object reference, 1) Reference to audio/video data
/// Returns: 1 on success, 0 if no video header was found
/// To Do: Properly parse MPEG streams:
///   0xb7 - sequence end
///   0xb9 - end code
///   0xba - pack start code
///   0xbb - system header
///   0xbc - program map <-- should parse this
///   0xbd - private stream 1 --> for VOB, this contains sub-streams:
///           0x20-0x3f - pictures
///           0x80-0x87 - audio (AC3,DTS,SDDS)
///           0xa0-0xa7 - audio (LPCM)
///   0xbe - padding
///   0xbf - private stream 2
///   0xc0-0xdf - audio stream
///   0xe0-0xef - video stream
pub fn parse_mpeg_audio_video(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, buff_pt) = (arg(&a, 0), arg(&a, 1));
    let (mut found_audio, mut found_video, mut did_hdr) = (false, false, false);
    let mut rtn_val = n(0);

    et.delete("AudioBitrate");
    et.delete("VideoBitrate");

    loop {
        let r = on_buff(it, r"$$buffPt =~ /\0\0\x01(\xb3|\xc0)/g ? ($1, pos($$buffPt)) : ()", &buff_pt)?;
        if r.is_empty() {
            break;
        }
        let (c1, p) = (arg(&r, 0), arg(&r, 1));
        let video = &c1.to_bytes()[..] == b"\xb3";
        if !did_hdr {
            // make sure we didn't miss an audio frame sync before this (eg. MP3 file)
            // (the last byte of the 4-byte MP3 audio frame header word may be zero,
            // but the 2nd last must be non-zero, so we need only check to pos-3)
            let buff = pl::substr(&buff_pt.deref(), 0, Some(pl::sub(&p, &n(3)).to_i64()));
            if first(parse_mpeg_audio(it, vec![et.clone(), pl::sref(buff)], Want::Scalar)?).truthy() {
                found_audio = true;
            }
            did_hdr = true;
        }
        if (video && found_video) || (!video && found_audio) {
            continue;
        }
        let mut len = pl::sub(&pl::length(&buff_pt.deref()), &p);
        if pl::lt(&len, &n(4)) {
            break;
        }
        if pl::gt(&len, &n(256)) {
            len = n(256);
        }
        let dat = pl::substr(&buff_pt.deref(), p.to_i64(), Some(len.to_i64()));
        // process MPEG audio or video
        let args = vec![et.clone(), pl::sref(dat)];
        let ok = if video { process_mpeg_video(it, args, Want::Scalar)? } else { parse_mpeg_audio(it, args, Want::Scalar)? };
        if first(ok).truthy() {
            rtn_val = n(1);
            if video {
                found_video = true;
            } else {
                found_audio = true;
            }
            // done if we found audio and video
            if found_audio && found_video {
                break;
            }
        }
    }
    ret(rtn_val)
}

/// Read information from an MPEG file
/// Inputs: 0) ExifTool object reference, 1) Directory information reference
/// Returns: 1 on success, 0 if this wasn't a valid MPEG file
pub fn process_mpeg(it: &mut Interp, a: Vec<Value>, want: Want) -> ER<Vec<Value>> {
    let (et, dir_info) = (arg(&a, 0), arg(&a, 1));
    let raf = dir_info.get("RAF");
    let buff = cell(Value::Undef);

    if !pl::num_eq(&raf_read_cell(it, &raf, &buff, n(4))?, &n(4)) {
        return ret(n(0));
    }
    if !re(r"^\0\0\x01[\xb0-\xbf]", "").is_match(&buff.borrow().clone()) {
        return ret(n(0));
    }
    m(it, &et, "SetFileType", vec![])?;

    m(it, &raf, "Seek", vec![n(0), n(0)])?;
    if !raf_read_cell(it, &raf, &buff, n(65536 * 4))?.truthy() {
        return ret(n(0));
    }

    parse_mpeg_audio_video(it, vec![et, Value::ScalarRef(buff)], want)
}

pub fn register(it: &mut Interp) {
    it.define_native("Image::ExifTool::MPEG::ProcessFrameHeader", process_frame_header);
    it.define_native("Image::ExifTool::MPEG::ParseMPEGAudio", parse_mpeg_audio);
    it.define_native("Image::ExifTool::MPEG::ProcessMPEGVideo", process_mpeg_video);
    it.define_native("Image::ExifTool::MPEG::ParseMPEGAudioVideo", parse_mpeg_audio_video);
    it.define_native("Image::ExifTool::MPEG::ProcessMPEG", process_mpeg);
}
