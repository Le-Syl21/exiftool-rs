//! Image::ExifTool::M2TS (Image/ExifTool/M2TS.pm), ported sub by sub (see docs/port/PORTING.md).
//!
//! Read M2TS (AVCHD) meta information
//!
//! The `m//g` matches on `$$dataPt` run as Perl statements (`gmatch`), so
//! pos() stays on the caller's scalar as in perl.

use crate::interp::eval::ER;
use crate::interp::{Interp, Want};
use crate::modules::random_access::{cell, perl_eval};
use crate::perl::Value;
use crate::pl::{self, arg, cap, n, re, ret};

const MODULE: &str = "Image::ExifTool::M2TS";

/// `Image::ExifTool::NAME(ARGS)` (the functions imported with :DataAccess :Utils).
fn call(it: &mut Interp, name: &str, args: Vec<Value>) -> ER<Value> {
    pl::call(it, &format!("Image::ExifTool::{name}"), args)
}

fn m(it: &mut Interp, et: &Value, name: &str, args: Vec<Value>) -> ER<Value> {
    pl::method(it, et, name, args)
}

fn handle(it: &mut Interp, et: &Value, tbl: &Value, tag: &str, val: Value) -> ER<()> {
    m(it, et, "HandleTag", vec![tbl.clone(), pl::s(tag), val])?;
    Ok(())
}

fn warn(it: &mut Interp, et: &Value, msg: &str) -> ER<()> {
    m(it, et, "Warn", vec![pl::s(msg)])?;
    Ok(())
}

fn cat(parts: &[&Value]) -> Value {
    let mut v = Vec::new();
    for p in parts {
        v.extend_from_slice(&p.to_bytes());
    }
    Value::bytes(v)
}

/// `print $out STRING`
fn out_print(it: &mut Interp, out: &Value, s: Value) -> ER<()> {
    pl::print_to(it, Some(out), &[s])?;
    Ok(())
}

/// `$$dataPt =~ /PAT/g` in scalar context (pos() kept on `$$dataPt`): the
/// captures `$1..$ncap` (index 1..) when it matched.
fn gmatch(it: &mut Interp, pat: &str, data_pt: &Value, ncap: usize) -> ER<Option<Vec<Value>>> {
    let caps: Vec<String> = (1..=ncap).map(|i| format!("${i}")).collect();
    let code = format!("$$dataPt =~ {pat} ? (1, {}) : ()", caps.join(", "));
    let c = cell(data_pt.clone());
    let r = perl_eval(it, &code, MODULE, &[("dataPt", &c)], Want::List)?;
    if r.is_empty() {
        return Ok(None);
    }
    Ok(Some(r))
}

fn lex_hash(it: &mut Interp, name: &str) -> Value {
    crate::runtime::lex_hash(it, MODULE, name)
}

fn knots_to_kph(it: &mut Interp) -> Value {
    crate::runtime::lex_scalar(it, MODULE, "knotsToKph").borrow().clone()
}

fn inc_doc(et: &Value) {
    let dc = pl::add(&et.get("DOC_COUNT"), &n(1));
    et.set("DOC_COUNT", dc.clone());
    et.set("DOC_NUM", dc);
}

/// `$s =~ s/([\x00-\x1f\x7f-\xff])/sprintf("\\x%.2x",ord $1)/eg`
fn escape_desc(s: &mut Value) {
    re(r"([\x00-\x1f\x7f-\xff])", "").subst(s, true, |c| format!("\\x{:02x}", cap(c, 1).to_bytes()[0]).into_bytes());
}

/// Extract information from AC-3 audio stream
/// Inputs: 0) ExifTool ref, 1) data ref
/// Reference: http://www.atsc.org/standards/a_52b.pdf
pub fn parse_ac3_audio(it: &mut Interp, a: Vec<Value>, want: Want) -> ER<Vec<Value>> {
    let (et, data_pt) = (arg(&a, 0), arg(&a, 1));
    if let Some(c) = gmatch(it, r"/\x0b\x77..(.)/sg", &data_pt, 1)? {
        let sample_rate = n((arg(&c, 1).to_bytes().first().copied().unwrap_or(0) as i64) >> 6);
        let tag_table_ptr = call(it, "GetTagTable", vec![pl::s("Image::ExifTool::M2TS::AC3")])?;
        return Ok(vec![m(it, &et, "HandleTag", vec![tag_table_ptr, pl::s("AudioSampleRate"), sample_rate])?]);
    }
    // (the value of the failed condition)
    Ok(if want == Want::List { vec![] } else { vec![pl::s("")] })
}

/// Extract information from AC-3 stream descriptor
/// Inputs: 0) ExifTool ref, 1) data ref
/// Reference: http://www.atsc.org/standards/a_52b.pdf
/// Note: This information is duplicated in the Audio stream, but it
///       is somewhat easier to extract it from the descriptor instead
pub fn parse_ac3_descriptor(it: &mut Interp, a: Vec<Value>, want: Want) -> ER<Vec<Value>> {
    let (et, data_pt) = (arg(&a, 0), arg(&a, 1));
    if pl::lt(&pl::length(&data_pt.deref()), &n(3)) {
        return pl::ret_nothing(want);
    }
    let v = pl::unpack("C3", &data_pt.deref())?;
    let (v1, v2) = (arg(&v, 1).to_i64(), arg(&v, 2).to_i64());
    let tag_table_ptr = call(it, "GetTagTable", vec![pl::s("Image::ExifTool::M2TS::AC3")])?;
    // $et->HandleTag($tagTablePtr, 'AudioSampleRate', $v[0] >> 5);
    handle(it, &et, &tag_table_ptr, "AudioBitrate", n(v1 >> 2))?;
    handle(it, &et, &tag_table_ptr, "SurroundMode", n(v1 & 0x03))?;
    let r = m(it, &et, "HandleTag", vec![tag_table_ptr, pl::s("AudioChannels"), n((v2 >> 1) & 0x0f)])?;
    // don't (yet) decode any more (language codes, etc)
    ret(r)
}

/// Parse PID stream data
/// Inputs: 0) ExifTool ref, 1) PID number, 2) PID type, 3) PID name, 4) data ref
/// Returns: 0=stream parsed OK,
///          1=stream parsed but we want to parse more of these,
///          -1=can't parse yet because we don't know the type
pub fn parse_pid(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, pid, type_, pid_name, data_pt) = (arg(&a, 0), arg(&a, 1), arg(&a, 2), arg(&a, 3), arg(&a, 4));
    // can't parse until we know the type (Program Map Table may be later in the stream)
    if type_.is_undef() {
        return ret(n(-1));
    }
    let verbose = m(it, &et, "Options", vec![pl::s("Verbose")])?;
    if pl::gt(&verbose, &n(1)) {
        let out = m(it, &et, "Options", vec![pl::s("TextOut")])?;
        let s = pl::sprintf("Parsing stream 0x%.4x (%s) %d bytes\n", &[pid.clone(), pid_name.clone(), pl::length(&data_pt.deref())]);
        out_print(it, &out, s)?;
        m(it, &et, "VerboseDump", vec![data_pt.clone()])?;
    }
    let mut more = n(0);
    let opts = et.get("OPTIONS");
    if pl::num_eq(&type_, &n(0x01)) || pl::num_eq(&type_, &n(0x02)) {
        // MPEG-1/MPEG-2 Video
        crate::runtime::require(it, "Image::ExifTool::MPEG")?;
        pl::call(it, "Image::ExifTool::MPEG::ParseMPEGAudioVideo", vec![et.clone(), data_pt.clone()])?;
    } else if pl::num_eq(&type_, &n(0x03)) || pl::num_eq(&type_, &n(0x04)) {
        // MPEG-1/MPEG-2 Audio
        crate::runtime::require(it, "Image::ExifTool::MPEG")?;
        pl::call(it, "Image::ExifTool::MPEG::ParseMPEGAudio", vec![et.clone(), data_pt.clone()])?;
    } else if pl::num_eq(&type_, &n(6)) && pl::num_eq(&pid, &n(0x0300)) {
        // LIGOGPSINFO
        // unknown dashcam (../testpics/gps_video/Wrong Way pass.ts, 160 bytes, not fuzzed)
        // Pruveeo D90 dashcam (../testpics/gps_video/20260216_101326F.ts, 200 bytes, fuzzed)
        let data = data_pt.deref();
        if re(r"^LIGOGPSINFO", "s").is_match(&data) {
            let tbl = call(it, "GetTagTable", vec![pl::s("Image::ExifTool::QuickTime::Stream")])?;
            let dir_info = Value::new_hash();
            dir_info.set("DataPt", data_pt.clone());
            dir_info.set("DirName", pl::s("Ligo0x0300"));
            let flag = Value::from(!pl::num_eq(&pl::length(&data), &n(200)));
            pl::call(it, "Image::ExifTool::LigoGPS::ProcessLigoGPS", vec![et.clone(), dir_info, tbl, flag])?;
            et.set("FoundGoodGPS", n(1));
            more = n(1);
        } else if let Some(c) = re(
            r"^Viidure(20\d{2})/(\d{2})/(\d{2}) (\d{2}:\d{2}:\d{2}) ([NS]):(\S+) ([EW]):(\S+) (\S+) km/h (\S+) (\S+) (\S+) x:(\S+) y:(\S+) z:(\S+)",
            "s",
        )
        .m(&data)
        {
            // (Innovv N2 dashcam)
            // 0000: 56 69 69 64 75 72 65 32 30 32 36 2f 30 34 2f 31 [Viidure2026/04/1]
            // 0010: 36 20 30 31 3a 30 31 3a 30 32 20 4e 3a 34 32 2e [6 01:01:02 N:42.]
            // 0020: 32 31 31 34 32 34 20 57 3a 38 38 2e 33 32 30 39 [211424 W:88.3209]
            // 0030: 37 35 20 38 39 2e 31 20 6b 6d 2f 68 20 32 36 37 [75 89.1 km/h 267]
            // 0040: 2e 33 38 20 32 33 37 2e 38 30 20 31 30 20 78 3a [.38 237.80 10 x:]
            // 0050: 2d 30 2e 30 30 31 20 79 3a 2d 30 2e 30 30 31 20 [-0.001 y:-0.001 ]
            // 0060: 7a 3a 2d 30 2e 30 30 31 00 00 00 00 00 00 00 00 [z:-0.001........]
            let tbl = call(it, "GetTagTable", vec![pl::s("Image::ExifTool::QuickTime::Stream")])?;
            inc_doc(&et);
            let time = cat(&[&cap(&c, 1), &pl::s(":"), &cap(&c, 2), &pl::s(":"), &cap(&c, 3), &pl::s(" "), &cap(&c, 4), &pl::s("Z")]);
            handle(it, &et, &tbl, "GPSDateTime", time)?;
            let sg = |v: Value, neg: &str| if v.eq_str(neg) { n(-1) } else { n(1) };
            handle(it, &et, &tbl, "GPSLatitude", pl::mul(&cap(&c, 6), &sg(cap(&c, 5), "S")))?;
            handle(it, &et, &tbl, "GPSLongitude", pl::mul(&cap(&c, 8), &sg(cap(&c, 7), "W")))?;
            handle(it, &et, &tbl, "GPSSpeed", cap(&c, 9))?;
            handle(it, &et, &tbl, "GPSTrack", cap(&c, 10))?;
            handle(it, &et, &tbl, "GPSAltitude", cap(&c, 11))?;
            // (not sure what $12 is -- was a constant "10" in my sample)
            handle(it, &et, &tbl, "Accelerometer", cat(&[&cap(&c, 13), &pl::s(" "), &cap(&c, 14), &pl::s(" "), &cap(&c, 15)]))?;
            et.set("FoundGoodGPS", n(1));
            more = n(1);
            et.delete("DOC_NUM");
        }
    } else if pl::num_eq(&type_, &n(0x1b)) {
        // H.264 Video
        crate::runtime::require(it, "Image::ExifTool::H264")?;
        more = pl::call(it, "Image::ExifTool::H264::ParseH264Video", vec![et.clone(), data_pt.clone()])?;
        // force parsing additional H264 frames with ExtractEmbedded option
        if opts.get("ExtractEmbedded").truthy() {
            more = n(1);
        } else if !opts.get("Validate").truthy() {
            m(it, &et, "Warn", vec![pl::s("The ExtractEmbedded option may find more tags in the video data"), n(7)])?;
        }
    } else if pl::num_eq(&type_, &n(0x81)) || pl::num_eq(&type_, &n(0x87)) || pl::num_eq(&type_, &n(0x91)) {
        // AC-3 audio
        parse_ac3_audio(it, vec![et.clone(), data_pt.clone()], Want::Void)?;
    } else if pl::num_eq(&type_, &n(0x15)) {
        // packetized metadata (look for MISB code starting after 5-byte header)
        if re(r"^.{5}\x06\x0e\x2b\x34", "s").is_match(&data_pt.deref()) {
            let tbl = call(it, "GetTagTable", vec![pl::s("Image::ExifTool::MISB::Main")])?;
            more = pl::call(it, "Image::ExifTool::MISB::ParseMISB", vec![et.clone(), data_pt.clone(), tbl])?;
            let ee = opts.get("ExtractEmbedded");
            if !ee.truthy() {
                more = n(0); // extract from only the first packet unless ExtractEmbedded is used
            } else if pl::gt(&ee, &n(2)) {
                more = n(1); // read past unknown 0x15 packets if ExtractEmbedded > 2
            }
        }
    // still have a lot of questions about how to decode this...
    // (see https://exiftool.org/forum/index.php?topic=16486 and ../testpics/gps_video/forum16486.ts)
    //    } elsif ($type == 6) {
    //        my @a = unpack('x17x2NNx2nx2nx2nx2Cx2a4x2a5x2Nx2Nx2nx2Nx2Nx2Nx2nx2nx2Nx2nx2n', $$dataPt . "        ");
    //        my $hi = shift @a;
    //        $a[0] = Image::ExifTool::ConvertUnixTime(($a[0] + $hi * 4294967296) * 1e-6, undef, 6);
    //        print "@a\n";
    //        $more = 1;
    } else if pl::lt(&type_, &n(0)) {
        more = parse_pid_gps(it, &et, &pid, &data_pt)?;
        et.delete("DOC_NUM");
    }
    ret(more)
}

/// The `$type < 0` branch of ParsePID (GPS from dashcam streams): the new `$more`.
fn parse_pid_gps(it: &mut Interp, et: &Value, pid: &Value, data_pt: &Value) -> ER<Value> {
    let mut more = n(0);
    let data = data_pt.deref();
    let qt_stream = "Image::ExifTool::QuickTime::Stream";
    if let Some(c) = re(r"^(.{164})?(.{24})A[NS][EW]", "s").m(&data) {
        // (Blueskysea B4K, Novatek NT96670)
        // 0000: 01 00 ff 00 30 31 32 33 34 35 37 38 61 62 63 64 [....01234578abcd]
        // 0010: 65 66 67 0a 00 00 00 00 00 00 00 00 00 00 00 00 [efg.............]
        // 0020: 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 [................]
        // 0030: 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 [................]
        // 0040: 00 00 00 00 30 31 32 33 34 35 37 38 71 77 65 72 [....01234578qwer]
        // 0050: 74 79 75 69 6f 70 0a 00 00 00 00 00 00 00 00 00 [tyuiop..........]
        // 0060: 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 [................]
        // 0070: 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 [................]
        // 0080: 00 00 00 00 63 38 61 61 32 35 63 66 34 35 65 65 [....c8aa25cf45ee]
        // 0090: 61 39 65 32 34 34 32 66 61 65 62 35 65 30 39 39 [a9e2442faeb5e099]
        // 00a0: 30 37 64 34 15 00 00 00 10 00 00 00 1b 00 00 00 [07d4............]
        // 00b0: 15 00 00 00 01 00 00 00 09 00 00 00 41 4e 57 00 [............ANW.]
        // 00c0: 82 9a 57 45 98 b2 00 46 66 66 e4 41 d7 e3 14 43 [..WE...Fff.A...C]
        // 00d0: 01 00 02 00 03 00 04 00 05 00 06 00             [............]
        // (Viofo A119V3)
        // 0000: 08 00 00 00 07 00 00 00 18 00 00 00 15 00 00 00 [................]
        // 0010: 03 00 00 00 0b 00 00 00 41 4e 45 00 01 f2 ac 45 [........ANE....E]
        // 0020: 2d 7f 6e 45 b8 1e 97 41 d7 23 46 43 00 00 00 00 [-.nE...A.#FC....]
        // pad with dummy header and parse with existing FreeGPS code (minimum 92 bytes)
        let c1 = cap(&c, 1);
        let skip = pl::length(&if c1.truthy() { c1 } else { pl::s("") }).to_i64();
        let dat = cat(&[&Value::bytes([0u8; 16]), &pl::substr(&data, skip, None), &Value::bytes([0u8; 20])]);
        let tbl = call(it, "GetTagTable", vec![pl::s(qt_stream)])?;
        let dir_info = Value::new_hash();
        dir_info.set("DataPt", pl::sref(dat));
        pl::call(it, "Image::ExifTool::QuickTime::ProcessFreeGPS", vec![et.clone(), dir_info, tbl])?;
        more = n(1);
    } else if re(r"^(V00|A([NS])([EW]))\0", "s").is_match(&data) {
        // INNOVV TS video (same format as INNOVV MP4)
        call(it, "SetByteOrder", vec![pl::s("II")])?;
        let tag_tbl = call(it, "GetTagTable", vec![pl::s(qt_stream)])?;
        let kk = knots_to_kph(it);
        while let Some(c) = gmatch(it, r"/((V00|A[NS][EW])\0.{28})/g", data_pt, 2)? {
            let dat = arg(&c, 1);
            inc_doc(et);
            if !arg(&c, 2).eq_str("V00") {
                let mut lat = pl::abs(&call(it, "GetFloat", vec![pl::sref(dat.clone()), n(4)])?); // (abs just to be safe)
                let mut lon = pl::abs(&call(it, "GetFloat", vec![pl::sref(dat.clone()), n(8)])?); // (abs just to be safe)
                let spd = pl::mul(&call(it, "GetFloat", vec![pl::sref(dat.clone()), n(12)])?, &kk);
                let trk = call(it, "GetFloat", vec![pl::sref(dat.clone()), n(16)])?;
                let mut ll = [lat, lon];
                pl::call_mut(it, "Image::ExifTool::QuickTime::ConvertLatLon", &mut ll, Want::Scalar)?;
                [lat, lon] = ll;
                let s1 = if pl::substr(&dat, 1, Some(1)).eq_str("S") { n(-1) } else { n(1) };
                let s2 = if pl::substr(&dat, 2, Some(1)).eq_str("W") { n(-1) } else { n(1) };
                handle(it, et, &tag_tbl, "GPSLatitude", pl::mul(&pl::abs(&lat), &s1))?;
                handle(it, et, &tag_tbl, "GPSLongitude", pl::mul(&pl::abs(&lon), &s2))?;
                handle(it, et, &tag_tbl, "GPSSpeed", spd)?;
                handle(it, et, &tag_tbl, "GPSSpeedRef", pl::s("K"))?;
                handle(it, et, &tag_tbl, "GPSTrack", trk)?;
                handle(it, et, &tag_tbl, "GPSTrackRef", pl::s("T"))?;
            }
            let mut acc = pl::unpack("x20V3", &dat)?;
            for x in acc.iter_mut() {
                if pl::ge(x, &n(0x80000000)) {
                    *x = pl::sub(x, &n(4294967296));
                }
            }
            handle(it, et, &tag_tbl, "Accelerometer", pl::join(" ", &acc))?;
        }
        call(it, "SetByteOrder", vec![pl::s("MM")])?;
        et.set("FoundGoodGPS", n(1)); // (necessary to skip over empty/unknown INNOV records)
        more = n(1);
    } else if re(r"^\$(GPSINFO|GSNRINFO),", "").is_match(&data) {
        // $GPSINFO,0x0004,2021.08.09 13:27:36,2341.54561,12031.70135,8.0,51,153,0,0,\x0d
        // $GSNRINFO,0.01,0.04,0.25\0
        {
            let mut b = data_pt.deref().to_bytes().to_vec();
            for x in b.iter_mut() {
                if *x == 0x0d {
                    *x = 0x0a;
                }
            }
            b.retain(|&x| x != 0);
            pl::set_deref(data_pt, Value::bytes(b))?;
        }
        let tag_tbl = call(it, "GetTagTable", vec![pl::s(qt_stream)])?;
        let lines = pl::split(&re(r"\x0a", ""), &data_pt.deref(), 0);
        let mut last_time = Value::Undef;
        for line in lines.iter() {
            if re(r"^\$GPSINFO", "").is_match(line) {
                let mut av = pl::split(&re(",", ""), &arg(&lines, 0), 0);
                if av.len() <= 7 {
                    continue;
                }
                // ignore duplicate fixes
                if last_time.truthy() && av[2].to_bytes() == last_time.to_bytes() {
                    continue;
                }
                last_time = av[2].clone();
                inc_doc(et);
                pl::tr(&mut av[2], ".", ":", "");
                // (untested, and probably doesn't work for S/W hemispheres)
                let mut ll = [av[3].clone(), av[4].clone()];
                pl::call_mut(it, "Image::ExifTool::QuickTime::ConvertLatLon", &mut ll, Want::Scalar)?;
                let [lat, lon] = ll;
                // $a[0] - flags? values: '0x0001','0x0004','0x0008','0x0010'
                handle(it, et, &tag_tbl, "GPSDateTime", av[2].clone())?;
                handle(it, et, &tag_tbl, "GPSLatitude", lat)?;
                handle(it, et, &tag_tbl, "GPSLongitude", lon)?;
                handle(it, et, &tag_tbl, "GPSSpeed", av[5].clone())?;
                handle(it, et, &tag_tbl, "GPSSpeedRef", pl::s("K"))?;
                // $a[6] - values: 48-60
                handle(it, et, &tag_tbl, "GPSTrack", av[7].clone())?;
                handle(it, et, &tag_tbl, "GPSTrackRef", pl::s("T"))?;
                // #a[8,9] - always 0
            } else if re(r"^\$GSNRINFO", "").is_match(line) {
                let mut av = pl::split(&re(",", ""), line, 0);
                if !av.is_empty() {
                    av.remove(0);
                }
                handle(it, et, &tag_tbl, "Accelerometer", pl::join(" ", &av))?;
            }
        }
        more = n(1);
    } else if re(r"\$GPRMC,", "").is_match(&data) {
        // Jomise T860S-GM dashcam
        // $GPRMC,hhmmss.ss,A,ddmm.mmmmm,N,dddmm.mmmmm,W,spd-kts,dir-dg,DDMMYY,,M*cs - lat,lon,spd from video
        // $GPRMC,172255.00,A,:985.95194,N,17170.14674,W,029.678,170.68,240822,,,D*7B - N47.70428,W122.15338,35mph
        // $GPRMC,192643.00,A,:987.94979,N,17171.07268,W,010.059,079.61,111122,,,A*73 - N47.71862,W122.16437,12mph
        // $GPRMC,192743.00,A,:988.72110,N,17171.04873,W,017.477,001.03,111122,,,A*78 - N47.72421,W122.16408,20mph
        // $GPRMC,192844.00,A,:989.43771,N,17171.03538,W,016.889,001.20,111122,,,A*7B - N47.72932,W122.16393,19mph
        // $GPRMC,005241.00,A,:987.70873,N,17171.81293,W,000.284,354.78,141122,,,A*7F - N47.71687,W122.17318,0mph
        // $GPRMC,005341.00,A,:987.90851,N,17171.85380,W,000.080,349.36,141122,,,A*7C - N47.71832,W122.17367,0mph
        // $GPRMC,005441.00,A,:987.94538,N,17171.21783,W,029.686,091.09,141122,,,A*7A - N47.71859,W122.16630,35mph
        // $GPRMC,002816.00,A,6820.67273,N,13424.26599,W,000.045,000.00,261122,,,A*79 - N29.52096,W95.55953,0mph (seattle)
        // $GPRMC,035136.00,A,:981.47322,N,17170.14105,W,024.594,180.50,291122,,,D*79 - N47.67180,W122.15328,28mph
        let tag_tbl = call(it, "GetTagTable", vec![pl::s(qt_stream)])?;
        let kk = knots_to_kph(it);
        loop {
            let pat = r"/\$[A-Z]{2}RMC,(\d{2})(\d{2})(\d+(\.\d*)?),A?,(.{2})(\d{2}\.\d+),([NS]),(.{3})(\d{2}\.\d+),([EW]),(\d*\.?\d*),(\d*\.?\d*),(\d{2})(\d{2})(\d+)/g";
            let Some(c) = gmatch(it, pat, data_pt, 15)? else { break };
            // do some basic sanity checks on the date
            if !(pl::le(&c[13], &n(31)) && pl::le(&c[14], &n(12)) && pl::le(&c[15], &n(99))) {
                break;
            }
            inc_doc(et);
            let year = pl::add(&c[15], &n(if pl::ge(&c[15], &n(70)) { 1900 } else { 2000 }));
            let dt = pl::sprintf("%.4d:%.2d:%.2d %.2d:%.2d:%.2dZ", &[year, c[14].clone(), c[13].clone(), c[1].clone(), c[2].clone(), c[3].clone()]);
            handle(it, et, &tag_tbl, "GPSDateTime", dt)?;
            //(not this simple)
            //$et->HandleTag($tagTbl, GPSLatitude => (($5 || 0) + $6/60) * ($7 eq 'N' ? 1 : -1));
            //$et->HandleTag($tagTbl, GPSLongitude => (($8 || 0) + $9/60) * ($10 eq 'E' ? 1 : -1));
            if pl::length(&c[11]).to_i64() > 0 {
                handle(it, et, &tag_tbl, "GPSSpeed", pl::mul(&c[11], &kk))?;
            }
            if pl::length(&c[12]).to_i64() > 0 {
                handle(it, et, &tag_tbl, "GPSTrack", c[12].clone())?;
            }
            // it looks like maybe the degrees are xor-ed with something,
            // and the minutes have some scaling factor and offset?
            // (the code below is approximately correct for my only sample)
            let mut chars = pl::unpack("C*", &pl::concat(&c[5], &c[8]))?;
            let mut xor: Vec<i64> = vec![0x0e, 0x0e, 0x00, 0x05, 0x03]; // (empirical based on 1 sample; may be completely off base)
            let mut bad = false;
            for ch in chars.iter_mut() {
                let x = if xor.is_empty() { 0 } else { xor.remove(0) };
                *ch = n(ch.to_i64() ^ x);
                if ch.to_i64() < 0x30 || ch.to_i64() > 0x39 {
                    bad = true;
                }
            }
            if bad {
                warn(it, et, "Error decrypting GPS degrees")?;
            } else {
                let la = pl::pack("C*", &[arg(&chars, 0), arg(&chars, 1)])?;
                let lo = pl::pack("C*", &[arg(&chars, 2), arg(&chars, 3), arg(&chars, 4)])?;
                warn(it, et, "Decryption of this GPS is highly experimental. More testing samples are required")?;
                let or0 = |v: &Value| if v.truthy() { v.clone() } else { n(0) };
                let lat_m = pl::add(&pl::div(&pl::sub(&c[6], &Value::Num(85.95194)), &Value::Num(2.43051724137931))?, &Value::Num(42.2568));
                let lat = pl::mul(&pl::add(&or0(&la), &pl::div(&lat_m, &n(60))?), &if c[7].eq_str("N") { n(1) } else { n(-1) });
                handle(it, et, &tag_tbl, "GPSLatitude", lat)?;
                let lon_m = pl::add(&pl::div(&pl::sub(&c[9], &Value::Num(70.14674)), &Value::Num(1.460987654320988))?, &Value::Num(9.2028));
                let lon = pl::mul(&pl::add(&or0(&lo), &pl::div(&lon_m, &n(60))?), &if c[10].eq_str("E") { n(1) } else { n(-1) });
                handle(it, et, &tag_tbl, "GPSLongitude", lon)?;
            }
        }
    } else if let Some(c) = re(r"\$GSENSORD,\s*(\d+),\s*(\d+),\s*(\d+),", "").m(&data) {
        // Jomise T860S-GM dashcam
        let tag_tbl = call(it, "GetTagTable", vec![pl::s(qt_stream)])?;
        et.set("DOC_NUM", et.get("DOC_COUNT"));
        let acc = cat(&[&cap(&c, 1), &pl::s(" "), &cap(&c, 2), &pl::s(" "), &cap(&c, 3)]);
        handle(it, et, &tag_tbl, "Accelerometer", acc)?; // (NC - values range from 0 to 6)
    } else if let Some(c) = re(r"^.{44}A\0{3}.{4}([NS])\0{3}.{4}([EW])\0{3}", "s").m(&data).filter(|_| pl::ge(&pl::length(&data), &n(84))) {
        //forum11320
        call(it, "SetByteOrder", vec![pl::s("II")])?;
        let tag_tbl = call(it, "GetTagTable", vec![pl::s(qt_stream)])?;
        let lat = pl::abs(&call(it, "GetFloat", vec![data_pt.clone(), n(48)])?); // (abs just to be safe)
        let lon = pl::abs(&call(it, "GetFloat", vec![data_pt.clone(), n(56)])?); // (abs just to be safe)
        let spd = call(it, "GetFloat", vec![data_pt.clone(), n(64)])?;
        let trk = call(it, "GetFloat", vec![data_pt.clone(), n(68)])?;
        warn(it, et, "GPSLatitude/Longitude encryption is not yet known, so these will be wrong")?;
        inc_doc(et);
        let mut date = pl::unpack("x32V3x28V3", &data_pt.deref())?;
        if date.len() < 4 {
            date.resize(4, Value::Undef);
        }
        date[3] = pl::add(&date[3], &n(2000));
        let d = |i: usize| arg(&date, i);
        let dt = pl::sprintf("%.4d:%.2d:%.2d %.2d:%.2d:%.2d", &[d(3), d(4), d(5), d(0), d(1), d(2)]);
        handle(it, et, &tag_tbl, "GPSDateTime", dt)?;
        handle(it, et, &tag_tbl, "GPSLatitude", pl::mul(&pl::abs(&lat), &if cap(&c, 1).eq_str("S") { n(-1) } else { n(1) }))?;
        handle(it, et, &tag_tbl, "GPSLongitude", pl::mul(&pl::abs(&lon), &if cap(&c, 2).eq_str("W") { n(-1) } else { n(1) }))?;
        handle(it, et, &tag_tbl, "GPSSpeed", spd)?;
        handle(it, et, &tag_tbl, "GPSSpeedRef", pl::s("K"))?;
        handle(it, et, &tag_tbl, "GPSTrack", trk)?;
        handle(it, et, &tag_tbl, "GPSTrackRef", pl::s("T"))?;
        call(it, "SetByteOrder", vec![pl::s("MM")])?;
        more = n(1);
    } else if pl::length(&data).to_i64() >= 64 && pl::substr(&data, 32, Some(2)).eq_str("$S") {
        // DOD_LS600W.TS
        let tag_tbl = call(it, "GetTagTable", vec![pl::s(qt_stream)])?;
        // find the earliest sample time in the cyclical list
        let (mut nn, mut last) = (32i64, Value::bytes([0u8]));
        let dlen = pl::length(&data).to_i64();
        let mut i = 32;
        while i < dlen - 32 {
            if !pl::substr(&data, nn, Some(2)).eq_str("$S") {
                break;
            }
            let date_time = pl::substr(&data, i + 6, Some(8));
            if last.to_bytes() > date_time.to_bytes() {
                nn = i;
                break; // earliest sample if time goes backwards
            }
            last = date_time;
            i += 32;
        }
        let mut i = 32;
        while i < dlen - 32 {
            if nn > dlen - 32 {
                nn = 32;
            }
            if !pl::substr(&data, nn, Some(2)).eq_str("$S") {
                break;
            }
            let mut av = pl::unpack(&format!("x{nn}nnnnCCCCnCNNC"), &data)?;
            av.resize(av.len().max(13), Value::Undef);
            av[8] = pl::div(&av[8], &n(10))?; // 1/10 sec
            if av[2].to_i64() & 0x8000 != 0 {
                av[2] = pl::add(&av[2], &n(36000 - 65536)); // convert signed integer into range 0-36000
            }
            inc_doc(et);
            let dt = pl::sprintf("%.4d:%.2d:%.2d %.2d:%.2d:%04.1fZ", &av[3..9]);
            handle(it, et, &tag_tbl, "GPSDateTime", dt)?;
            handle(it, et, &tag_tbl, "GPSLatitude", pl::mul(&av[10], &Value::Num(1e-7)))?;
            handle(it, et, &tag_tbl, "GPSLongitude", pl::mul(&av[11], &Value::Num(1e-7)))?;
            handle(it, et, &tag_tbl, "GPSSpeed", pl::mul(&av[1], &Value::Num(0.036)))?; // convert from metres per 100 s
            handle(it, et, &tag_tbl, "GPSTrack", pl::div(&av[2], &n(100))?)?;
            i += 32;
            nn += 32;
        }
        // Note: 10 bytes after last GPS record look like a single 3-axis accelerometer reading:
        // eg. fd ff 00 00 ff ff 00 00 01 00
        et.set("FoundGoodGPS", n(1)); // so we skip over unrecognized packets
        more = n(1);
    } else if re(r"^skip.{4}LIGOGPSINFO\0", "s").is_match(&data) {
        // (this record contains 2 copies of the same 'skip' atom in my sample --
        //  only extract data from the first one)
        let tbl = call(it, "GetTagTable", vec![pl::s(qt_stream)])?;
        let dir_info = Value::new_hash();
        dir_info.set("DataPt", data_pt.clone());
        dir_info.set("DirStart", n(8));
        dir_info.set("DirName", pl::sprintf("Ligo0x%.4x", &[pid.clone()]));
        pl::call(it, "Image::ExifTool::LigoGPS::ProcessLigoGPS", vec![et.clone(), dir_info, tbl, n(1)])?;
        et.set("FoundGoodGPS", n(1));
    } else if et.get("FoundGoodGPS").truthy() {
        more = n(1);
    }
    Ok(more)
}

fn parse_pid1(it: &mut Interp, et: &Value, pid: &Value, pid_type: &Value, pid_name: &Value, data: &Value) -> ER<Value> {
    let key = pid.to_bytes();
    let args = vec![et.clone(), pid.clone(), pid_type.get(&key), pid_name.get(&key), pl::elem_ref(data, &key)];
    Ok(parse_pid(it, args, Want::Scalar)?.pop().unwrap_or_default())
}

/// `$raf->Read($buff, $len)`: the count, with `buff` updated.
fn read(it: &mut Interp, raf: &Value, buff: &mut Value, len: Value) -> ER<Value> {
    let mut args = [buff.clone(), len];
    let r = pl::method_mut(it, raf, "Read", &mut args, Want::Scalar)?;
    *buff = args[0].clone();
    Ok(r.into_iter().next().unwrap_or_default())
}

fn get8u(it: &mut Interp, buf: &Value, pos: i64) -> ER<i64> {
    Ok(call(it, "Get8u", vec![pl::sref(buf.clone()), n(pos)])?.to_i64())
}

fn get16u(it: &mut Interp, buf: &Value, pos: i64) -> ER<i64> {
    Ok(call(it, "Get16u", vec![pl::sref(buf.clone()), n(pos)])?.to_i64())
}

fn get32u(it: &mut Interp, buf: &Value, pos: i64) -> ER<Value> {
    call(it, "Get32u", vec![pl::sref(buf.clone()), n(pos)])
}

/// `$h{$k}` with an integer key.
fn hget(h: &Value, k: i64) -> Value {
    h.get(k.to_string())
}

fn hset(h: &Value, k: i64, v: Value) {
    h.set(k.to_string(), v)
}

fn hdel(h: &Value, k: i64) {
    h.delete(k.to_string());
}

/// Extract information from a M2TS file
/// Inputs: 0) ExifTool object reference, 1) DirInfo reference
/// Returns: 1 on success, 0 if this wasn't a valid M2TS file
pub fn process_m2ts(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info) = (arg(&a, 0), arg(&a, 1));
    let raf = dir_info.get("RAF");
    let mut buff = Value::Undef;
    let mut eof = false;
    let (pmt, pid_type, data, sect_len, pack_len, from_start) = (Value::new_hash(), Value::new_hash(), Value::new_hash(), Value::new_hash(), Value::new_hash(), Value::new_hash());
    let (mut start_time, mut end_time, mut fwd_time, mut back_scan, mut max_back) = (Value::Undef, Value::Undef, Value::Undef, Value::Undef, Value::Undef);
    let verbose = m(it, &et, "Options", vec![pl::s("Verbose")])?;
    let out = m(it, &et, "Options", vec![pl::s("TextOut")])?;
    let verb = verbose.truthy();
    let verb2 = pl::gt(&verbose, &n(1));
    let stream_type = lex_hash(it, "streamType");
    let table_id_h = lex_hash(it, "tableID");
    let no_syntax = lex_hash(it, "noSyntax");

    // read enough to guarantee 2 sync bytes
    if !pl::num_eq(&read(it, &raf, &mut buff, n(383))?, &n(383)) {
        return ret(n(0));
    }
    // test for magic number (sync byte is the only thing we can safely check)
    let Some(c) = re(r"^(.{0,190}?)\x47(.{187}|.{191})\x47", "s").m(&buff) else { return ret(n(0)) };
    let mut tc_len = pl::length(&cap(&c, 2)).to_i64() - 187; // (length of timecode = 0 or 4 bytes)
    let mut start = pl::length(&cap(&c, 1)).to_i64() - tc_len;
    let mut p_len;
    let read_size;
    // we may need to try the validation twice to handle the edge case
    // where the first byte of a timecode is 0x47 and we were fooled
    // into thinking there was no timecode
    'try_: loop {
        if start < 0 {
            start += 192; // (if all or part of first timecode was missing)
        }
        p_len = 188 + tc_len;
        let rs = 64 * p_len; // size of our read buffer
        m(it, &raf, "Seek", vec![n(start), n(0)])?; // rewind to start
        if !pl::ge(&read(it, &raf, &mut buff, n(rs))?, &n(p_len * 4)) {
            return ret(n(0)); // require at least 4 packets
        }
        // validate the sync byte in the next 3 packets
        for j in 1..4 {
            if pl::substr(&buff, tc_len + p_len * j, Some(1)).eq_str("G") {
                continue; // (0x47)
            }
            if tc_len != 0 {
                return ret(n(0));
            }
            tc_len = 4;
            start -= 4;
            continue 'try_;
        }
        read_size = rs;
        break; // success!
    }
    // (use M2T instead of M2TS just as an indicator that there is no timecode)
    m(it, &et, "SetFileType", vec![pl::s(if tc_len != 0 { "M2TS" } else { "M2T" })])?;
    if start != 0 {
        warn(it, &et, "File doesn't begin with the start of a packet")?;
    }
    call(it, "SetByteOrder", vec![pl::s("MM")])?;
    let tag_table_ptr = call(it, "GetTagTable", vec![pl::s("Image::ExifTool::M2TS::Main")])?;

    // PID lookup strings (will add to this with entries from program map table)
    let pid_name = Value::new_hash();
    hset(&pid_name, 0, pl::s("Program Association Table"));
    hset(&pid_name, 1, pl::s("Conditional Access Table"));
    hset(&pid_name, 2, pl::s("Transport Stream Description Table"));
    hset(&pid_name, 0x1fff, pl::s("Null Packet"));
    let did_pid = Value::new_hash();
    hset(&did_pid, 1, n(0));
    hset(&did_pid, 2, n(0));
    hset(&did_pid, 0x1fff, n(0));
    let need_pid = Value::new_hash(); // lookup for stream PID's that we still need to parse
    hset(&need_pid, 0, n(1));
    // PID's that may contain GPS info
    let gps_pid = Value::new_hash();
    hset(&gps_pid, 0x0300, n(1)); // Novatek INNOVV, DOD_LS600W
    hset(&gps_pid, 0x01e4, n(1)); // vsys a6l dashcam
    hset(&gps_pid, 0x0e1b, n(1)); // Jomise T860S-GM dashcam GPS
    hset(&gps_pid, 0x0e1a, n(1)); // Jomise T860S-GM dashcam accelerometer
    let mut p_end: i64 = 0;

    // scan entire file for GPS programs if ExtractEmbedded option is 3 or higher
    // (some dashcams write these programs but don't include it in the PMT)
    let ee = m(it, &et, "Options", vec![pl::s("ExtractEmbedded")])?;
    if pl::gt(&if ee.truthy() { ee } else { n(0) }, &n(2)) {
        for k in gps_pid.keys() {
            need_pid.set(&k, n(1));
            pid_type.set(&k, n(-1));
            pid_name.set(&k, pl::s("unregistered dashcam GPS"));
        }
    }

    // parse packets from MPEG-2 Transport Stream
    loop {
        if need_pid.is_empty() {
            if start_time.is_undef() {
                break;
            }
            // reconfigure to seek backwards for last PCR
            if back_scan.is_undef() {
                let save_time = end_time.clone();
                end_time = Value::Undef;
                if m(it, &et, "Options", vec![pl::s("FastScan")])?.truthy() {
                    break;
                }
                if verb {
                    out_print(it, &out, pl::s("[Starting backscan for last PCR]\n"))?;
                }
                // remember how far we got when reading forward through the file
                let tell = m(it, &raf, "Tell", vec![])?;
                let fwd_pos = pl::add(&pl::sub(&tell, &pl::length(&buff)), &n(p_end));
                // determine the position of the last packet relative to the EOF
                if !m(it, &raf, "Seek", vec![n(0), n(2)])?.truthy() {
                    break;
                }
                let fsize = m(it, &raf, "Tell", vec![])?;
                back_scan = pl::sub(&pl::mul(&pl::int(&pl::div(&fsize, &n(p_len))?), &n(p_len)), &fsize);
                // set limit on how far back we will go
                max_back = pl::sub(&fwd_pos, &fsize);
                // scan back a maximum of 512k (have seen last PCR at -276k)
                let n_max = pl::int(&pl::div(&n(512000), &n(p_len))?); // max packets to backscan
                if pl::lt(&n_max, &pl::int(&pl::div(&pl::neg(&max_back), &n(p_len))?)) {
                    max_back = pl::sub(&back_scan, &pl::mul(&n_max, &n(p_len)));
                } else {
                    // use this time if none found in all remaining packets
                    fwd_time = save_time;
                }
                p_end = 0;
            }
        }
        let mut pos: i64;
        // read more if necessary
        if !back_scan.is_undef() {
            if !end_time.is_undef() {
                break;
            }
            p_end -= 2 * p_len; // step back to previous packet
            pos = p_end;
            if pos < 0 {
                // read another buffer from end of file
                if pl::le(&back_scan, &max_back) {
                    break;
                }
                let mut buff_len = pl::sub(&back_scan, &max_back);
                if pl::gt(&buff_len, &n(read_size)) {
                    buff_len = n(read_size);
                }
                back_scan = pl::sub(&back_scan, &buff_len);
                if !m(it, &raf, "Seek", vec![back_scan.clone(), n(2)])?.truthy() {
                    break;
                }
                if !pl::num_eq(&read(it, &raf, &mut buff, buff_len.clone())?, &buff_len) {
                    break;
                }
                p_end = buff_len.to_i64() - p_len;
                pos = p_end;
            }
        } else {
            pos = p_end;
            if pos + p_len > pl::length(&buff).to_i64() {
                if !pl::ge(&read(it, &raf, &mut buff, n(read_size))?, &n(p_len)) {
                    eof = true;
                    break;
                }
                p_end = 0;
                pos = 0;
            }
        }
        p_end += p_len;
        // decode the packet prefix
        pos += tc_len;
        let prefix = pl::unpack1(&format!("x{pos}N"), &buff)?.to_i64(); // (use unpack instead of Get32u for speed)
        // validate sync byte
        if (prefix & 0xff000000) != 0x47000000 {
            if back_scan.is_undef() {
                warn(it, &et, "M2TS synchronization error")?;
            }
            break;
        }
        // my $transport_error_indicator    = $prefix & 0x00800000;
        let payload_unit_start_indicator = prefix & 0x00400000;
        // my $transport_priority           = $prefix & 0x00200000;
        let pid = (prefix & 0x001fff00) >> 8; // packet ID
        // my $transport_scrambling_control = $prefix & 0x000000c0;
        let adaptation_field_exists = prefix & 0x00000020;
        let payload_data_exists = prefix & 0x00000010;
        // my $continuity_counter           = $prefix & 0x0000000f;
        if verb2 {
            let tell = m(it, &raf, "Tell", vec![])?;
            let i = pl::sub(&pl::div(&pl::add(&pl::sub(&tell, &pl::length(&buff)), &n(p_end)), &n(p_len))?, &n(1));
            out_print(it, &out, cat(&[&pl::s("Transport packet "), &i, &pl::s(":\n")]))?;
            let addr = pl::mul(&i, &n(p_len));
            m(it, &et, "VerboseDump", vec![pl::sref(buff.clone()), pl::s("Len"), n(p_len), pl::s("Addr"), addr, pl::s("Start"), n(pos - tc_len)])?;
            let pn = hget(&pid_name, pid);
            let str_ = if pn.truthy() { cat(&[&pl::s(" ("), &pn, &pl::s(")")]) } else { pl::s(" <not in Program Map Table!>") };
            if p_len == 192 {
                let tc = get32u(it, &buff, pos - tc_len)?;
                out_print(it, &out, pl::sprintf("  Timecode:   0x%.4x\n", &[tc]))?;
            }
            out_print(it, &out, pl::sprintf_v(&cat(&[&pl::s("  Packet ID:  0x%.4x"), &str_, &pl::s("\n")]), &[n(pid)]))?;
            out_print(it, &out, pl::sprintf("  Start Flag: %s\n", &[pl::s(if payload_unit_start_indicator != 0 { "Yes" } else { "No" })]))?;
        }

        pos += 4;
        // handle adaptation field
        if adaptation_field_exists != 0 {
            let len = get8u(it, &buff, pos)?;
            pos += 1;
            if pos + len > p_end {
                warn(it, &et, "Invalid adaptation field length")?;
                break;
            }
            // read PCR value for calculation of Duration
            if len > 6 {
                let flags = get8u(it, &buff, pos)?;
                if flags & 0x10 != 0 {
                    // PCR_flag
                    // combine 33-bit program_clock_reference_base and 9-bit extension
                    let pcr_base = get32u(it, &buff, pos + 1)?;
                    let pcr_ext = get16u(it, &buff, pos + 5)?;
                    // ignore separate programs (PID's) and store just the
                    // first and last timestamps found in the file (is this OK?)
                    end_time = pl::add(&pl::mul(&n(300), &pl::add(&pl::mul(&n(2), &pcr_base), &n(pcr_ext >> 15))), &n(pcr_ext & 0x01ff));
                    if start_time.is_undef() {
                        start_time = end_time.clone();
                    }
                }
            }
            pos += len;
        }

        // all done with this packet unless it carries a payload
        // or if we are just looking for the last timestamp
        if !(payload_data_exists != 0 && back_scan.is_undef()) {
            continue;
        }

        // decode payload data
        if pid == 0 ||            // program association table
            !hget(&pmt, pid).is_undef()
        // program map table(s)
        {
            // must interpret pointer field if payload_unit_start_indicator is set
            let buf2;
            if payload_unit_start_indicator != 0 {
                // skip to start of section
                let pointer_field = get8u(it, &buff, pos)?;
                pos += 1 + pointer_field;
                if pos >= p_end {
                    warn(it, &et, "Bad pointer field")?;
                    break;
                }
                buf2 = pl::substr(&buff, p_end - p_len, Some(p_len));
                pos -= p_end - p_len;
            } else {
                // not the start of a section
                if !hget(&sect_len, pid).truthy() {
                    continue;
                }
                let more = pl::sub(&hget(&sect_len, pid), &pl::length(&hget(&data, pid))).to_i64();
                let mut size = p_len - pos;
                if size > more {
                    size = more;
                }
                hset(&data, pid, pl::concat(&hget(&data, pid), &pl::substr(&buff, pos, Some(size))));
                if size != more {
                    continue;
                }
                // we have the complete section now, so put into $buf2 for parsing
                buf2 = hget(&data, pid);
                pos = 0;
                hdel(&data, pid);
                hdel(&from_start, pid);
                hdel(&sect_len, pid);
            }
            let slen = pl::length(&buf2).to_i64(); // section length
            if pos + 8 > slen {
                warn(it, &et, "Truncated payload")?;
                break;
            }
            // validate table ID
            let table_id = get8u(it, &buf2, pos)?;
            let tn = hget(&table_id_h, table_id);
            let name = pl::concat(&if tn.truthy() { tn } else { pl::sprintf("Unknown (0x%x)", &[n(table_id)]) }, &pl::s(" Table"));
            let expected_id = if pid != 0 { 0x02 } else { 0x00 };
            if table_id != expected_id {
                if verb2 {
                    out_print(it, &out, cat(&[&pl::s("  (skipping "), &name, &pl::s(")\n")]))?;
                }
                hdel(&need_pid, pid);
                hset(&did_pid, pid, n(1));
                continue;
            }
            // validate section syntax indicator for parsed tables (PAT, PMT)
            let section_syntax_indicator = get8u(it, &buf2, pos + 1)? & 0xc0;
            if section_syntax_indicator != 0x80 {
                m(it, &et, "Warn", vec![pl::concat(&pl::s("Bad "), &name)])?;
                break;
            }
            let section_length = get16u(it, &buf2, pos + 1)? & 0x0fff;
            if section_length > 1021 {
                m(it, &et, "Warn", vec![cat(&[&pl::s("Invalid "), &name, &pl::s(" length")])])?;
                break;
            }
            if slen < section_length + 3 {
                // (3 bytes for table_id + section_length)
                // must wait until we have the full section
                hset(&data, pid, pl::substr(&buf2, pos, None));
                hset(&sect_len, pid, n(section_length + 3));
                continue;
            }
            let program_number = get16u(it, &buf2, pos + 3)?;
            let section_number = get8u(it, &buf2, pos + 6)?;
            let last_section_number = get8u(it, &buf2, pos + 7)?;
            if verb2 {
                out_print(it, &out, pl::s(&format!("  {} length: {}\n", name.to_text(), section_length)))?;
                if pid != 0 {
                    out_print(it, &out, pl::s(&format!("  Program No: {program_number}\n")))?;
                }
                if pid == 0 {
                    out_print(it, &out, pl::sprintf("  Stream ID:  0x%x\n", &[n(program_number)]))?;
                }
                out_print(it, &out, pl::s(&format!("  Section No: {section_number}\n")))?;
                out_print(it, &out, pl::s(&format!("  Last Sect.: {last_section_number}\n")))?;
            }
            let end = pos + section_length + 3 - 4; // (don't read 4-byte CRC)
            pos += 8;
            if pid == 0 {
                // decode PAT (Program Association Table)
                while pos <= end - 4 {
                    let program_number = get16u(it, &buf2, pos)?;
                    let program_map_pid = get16u(it, &buf2, pos + 2)? & 0x1fff;
                    hset(&pmt, program_map_pid, n(program_number)); // save our PMT PID's
                    let str_ = pl::s(&format!("Program {program_number} Map"));
                    hset(&pid_name, program_map_pid, str_.clone());
                    if !hget(&did_pid, program_map_pid).truthy() {
                        hset(&need_pid, program_map_pid, n(1));
                    }
                    if verb {
                        out_print(it, &out, pl::sprintf_v(&cat(&[&pl::s("  PID(0x%.4x) --> "), &str_, &pl::s("\n")]), &[n(program_map_pid)]))?;
                    }
                    pos += 4;
                }
            } else {
                // decode PMT (Program Map Table)
                if pos + 4 > slen {
                    warn(it, &et, "Truncated PMT")?;
                    break;
                }
                let pcr_pid = get16u(it, &buf2, pos)? & 0x1fff;
                let program_info_length = get16u(it, &buf2, pos + 2)? & 0x0fff;
                let str_ = pl::s(&format!("Program {program_number} Clock Reference"));
                hset(&pid_name, pcr_pid, str_.clone());
                if verb {
                    out_print(it, &out, pl::sprintf_v(&cat(&[&pl::s("  PID(0x%.4x) --> "), &str_, &pl::s("\n")]), &[n(pcr_pid)]))?;
                }
                pos += 4;
                if pos + program_info_length > slen {
                    warn(it, &et, "Truncated program info")?;
                    break;
                }
                // dump program information descriptors if verbose
                if verb2 {
                    let mut j = 0;
                    while j < program_info_length - 2 {
                        let descriptor_tag = get8u(it, &buf2, pos + j)?;
                        let descriptor_length = get8u(it, &buf2, pos + j + 1)?;
                        j += 2;
                        if j + descriptor_length > program_info_length {
                            break;
                        }
                        let mut desc = pl::substr(&buf2, pos + j, Some(descriptor_length));
                        j += descriptor_length;
                        escape_desc(&mut desc);
                        let fmt = cat(&[&pl::s("    Program Descriptor: Type=0x%.2x \""), &desc, &pl::s("\"\n")]);
                        out_print(it, &out, pl::sprintf_v(&fmt, &[n(descriptor_tag)]))?;
                    }
                }
                pos += program_info_length; // skip descriptors (for now)
                while pos <= end - 5 {
                    let stream_type_v = get8u(it, &buf2, pos)?;
                    let elementary_pid = get16u(it, &buf2, pos + 1)? & 0x1fff;
                    let es_info_length = get16u(it, &buf2, pos + 3)? & 0x0fff;
                    let mut str_ = hget(&stream_type, stream_type_v);
                    if !str_.truthy() {
                        str_ = pl::s(if stream_type_v < 0x7f { "Reserved" } else { "Private" });
                    }
                    str_ = pl::sprintf("%s (0x%.2x)", &[str_, n(stream_type_v)]);
                    str_ = cat(&[&pl::s(&format!("Program {program_number} ")), &str_]);
                    if verb {
                        out_print(it, &out, pl::sprintf_v(&cat(&[&pl::s("  PID(0x%.4x) --> "), &str_, &pl::s("\n")]), &[n(elementary_pid)]))?;
                    }
                    if let Some(c) = re(r"(Audio|Video)", "").m(&str_) {
                        if !hget(&pid_name, elementary_pid).truthy() {
                            m(it, &et, "HandleTag", vec![tag_table_ptr.clone(), pl::concat(&cap(&c, 1), &pl::s("StreamType")), n(stream_type_v)])?;
                        }
                        // we want to parse all Audio and Video streams
                        if !hget(&did_pid, elementary_pid).truthy() {
                            hset(&need_pid, elementary_pid, n(1));
                        }
                    }
                    // save PID type and name string
                    hset(&pid_name, elementary_pid, str_.clone());
                    hset(&pid_type, elementary_pid, n(stream_type_v));
                    pos += 5;
                    if pos + es_info_length > slen {
                        warn(it, &et, "Truncated ES info")?;
                        pos = end;
                        break;
                    }
                    // parse elementary stream descriptors
                    let mut j = 0;
                    while j < es_info_length - 2 {
                        let descriptor_tag = get8u(it, &buf2, pos + j)?;
                        let descriptor_length = get8u(it, &buf2, pos + j + 1)?;
                        j += 2;
                        if j + descriptor_length > es_info_length {
                            break;
                        }
                        let desc = pl::substr(&buf2, pos + j, Some(descriptor_length));
                        j += descriptor_length;
                        if verb2 {
                            let mut dstr = desc.clone();
                            escape_desc(&mut dstr);
                            let fmt = cat(&[&pl::s("    ES Descriptor: Type=0x%.2x \""), &dstr, &pl::s("\"\n")]);
                            out_print(it, &out, pl::sprintf_v(&fmt, &[n(descriptor_tag)]))?;
                        }
                        // parse type-specific descriptor information (once)
                        if !hget(&did_pid, pid).truthy() && descriptor_tag == 0x81 {
                            // AC-3
                            parse_ac3_descriptor(it, vec![et.clone(), pl::sref(desc)], Want::Void)?;
                        }
                    }
                    pos += es_info_length;
                }
                let _ = pos;
            }
            // $pos = $end + 4; # skip CRC
        } else if hget(&did_pid, pid).is_undef() {
            let pv = n(pid);
            // save data from the start of each elementary stream
            if payload_unit_start_indicator != 0 {
                if !hget(&data, pid).is_undef() {
                    // we must have a whole section, so parse now
                    let more = parse_pid1(it, &et, &pv, &pid_type, &pid_name, &data)?;
                    // start fresh even if we couldn't process this PID yet
                    hdel(&data, pid);
                    hdel(&from_start, pid);
                    if !more.truthy() {
                        hdel(&need_pid, pid);
                        hset(&did_pid, pid, n(1));
                        continue;
                    }
                    // set flag indicating we found this PID but we still want more
                    hset(&need_pid, pid, n(-1));
                }
                // check for a PES header
                if pos + 6 > p_end {
                    continue;
                }
                let start_code = get32u(it, &buff, pos)?.to_i64();
                if (start_code & 0xffffff00) != 0x00000100 {
                    continue;
                }
                let stream_id = start_code & 0xff;
                let pes_packet_length = get16u(it, &buff, pos + 4)?;
                if verb2 {
                    out_print(it, &out, pl::sprintf("  Stream ID:  0x%.2x\n", &[n(stream_id)]))?;
                    out_print(it, &out, pl::s(&format!("  Packet Len: {pes_packet_length}\n")))?;
                }
                pos += 6;
                if !hget(&no_syntax, stream_id).truthy() {
                    if pos + 3 > p_end {
                        continue;
                    }
                    // validate PES syntax
                    let syntax = get8u(it, &buff, pos)? & 0xc0;
                    if syntax != 0x80 {
                        warn(it, &et, "Bad PES syntax")?;
                        continue;
                    }
                    // skip PES header
                    let pes_header_data_length = get8u(it, &buff, pos + 2)?;
                    pos += 3 + pes_header_data_length;
                    if pos >= p_end {
                        continue;
                    }
                }
                hset(&data, pid, pl::substr(&buff, pos, Some(p_end - pos)));
                // set flag that we read this payload from the start
                hset(&from_start, pid, n(1));
                // save the packet length
                if pes_packet_length > 8 {
                    hset(&pack_len, pid, n(pes_packet_length - 8)); // (where are the 8 extra bytes? - PH)
                } else {
                    hdel(&pack_len, pid);
                }
            } else {
                if hget(&data, pid).is_undef() {
                    // (vsys a6l dashcam GPS record doesn't have a start indicator)
                    if !hget(&gps_pid, pid).truthy() {
                        continue;
                    }
                    hset(&data, pid, pl::s(""));
                }
                // accumulate data for each elementary stream
                hset(&data, pid, pl::concat(&hget(&data, pid), &pl::substr(&buff, pos, Some(p_end - pos))));
            }
            // save only the first 256 bytes of most streams, except for
            // unknown, H.264 or metadata streams where we save up to 1 kB
            let mut save_len;
            let pt = hget(&pid_type, pid);
            if !pt.truthy() || pl::num_eq(&pt, &n(0x1b)) {
                save_len = n(1024);
            } else if pl::num_eq(&pt, &n(0x15)) {
                // use 1024 or actual size of metadata packet if smaller
                save_len = n(1024);
                let pl_ = hget(&pack_len, pid);
                if !pl_.is_undef() && pl::gt(&save_len, &pl_) {
                    save_len = pl_;
                }
            } else {
                save_len = n(256);
            }
            if pl::ge(&pl::length(&hget(&data, pid)), &save_len) {
                let mut more = parse_pid1(it, &et, &pv, &pid_type, &pid_name, &data)?;
                if pl::lt(&more, &n(0)) {
                    continue; // wait for program map table (hopefully not too long)
                }
                // don't stop parsing if we weren't successful and may have missed the start
                if !more.truthy() && !hget(&from_start, pid).truthy() {
                    more = n(1);
                }
                hdel(&data, pid);
                hdel(&from_start, pid);
                if more.truthy() {
                    hset(&need_pid, pid, n(-1));
                    continue; // parse more of these
                }
                hdel(&need_pid, pid);
                hset(&did_pid, pid, n(1));
            }
            continue;
        }
        if hget(&need_pid, pid).truthy() {
            // we found and parsed a section with this PID, so
            // delete from the lookup of PID's we still need to parse
            hdel(&need_pid, pid);
            hset(&did_pid, pid, n(1));
        }
    }

    // calculate Duration if available
    if end_time.is_undef() {
        end_time = fwd_time;
    }
    if !start_time.is_undef() && !end_time.is_undef() {
        if pl::gt(&start_time, &end_time) {
            end_time = pl::add(&end_time, &pl::mul(&n(0x80000000), &n(1200))); // handle 33-bit wrap
        }
        m(it, &et, "HandleTag", vec![tag_table_ptr.clone(), pl::s("Duration"), pl::sub(&end_time, &start_time)])?;
    }

    if verb {
        let mut need: Vec<Value> = Vec::new();
        for k in need_pid.keys() {
            if pl::gt(&need_pid.get(&k), &n(0)) {
                need.push(pl::sprintf("0x%.2x", &[Value::bytes(&k[..])]));
            }
        }
        if !need.is_empty() {
            need.sort_by(|x, y| x.to_bytes().cmp(&y.to_bytes()));
            out_print(it, &out, cat(&[&pl::s("End of file.  Missing PID(s): "), &pl::join(" ", &need), &pl::s("\n")]))?;
        } else {
            let what = if eof { "of file" } else { "scan" };
            out_print(it, &out, pl::s(&format!("End {what}.  All PID's parsed.\n")))?;
        }
    }

    // parse any remaining partial PID streams
    let mut keys: Vec<Value> = data.keys().into_iter().map(Value::from).collect();
    keys.sort_by(|x, y| x.to_bytes().cmp(&y.to_bytes()));
    for pid in keys {
        let k = pid.to_bytes();
        let args = vec![et.clone(), pid.clone(), pid_type.get(&k), pid_name.get(&k), pl::elem_ref(&data, &k)];
        parse_pid(it, args, Want::Scalar)?;
        data.delete(&k);
    }

    // look for LIGOGPSINFO trailer
    if m(it, &et, "Options", vec![pl::s("ExtractEmbedded")])?.truthy()
        && m(it, &raf, "Seek", vec![n(-8), n(2)])?.truthy()
        && pl::num_eq(&read(it, &raf, &mut buff, n(8))?, &n(8))
        && re(r"^&&&&", "").is_match(&buff)
    {
        let len = pl::unpack1("x4N", &buff)?;
        if pl::lt(&len, &m(it, &raf, "Tell", vec![])?) && m(it, &raf, "Seek", vec![pl::neg(&len), n(2)])?.truthy() && pl::num_eq(&read(it, &raf, &mut buff, len.clone())?, &len) {
            let tbl = call(it, "GetTagTable", vec![pl::s("Image::ExifTool::QuickTime::Stream")])?;
            let dir_info = Value::new_hash();
            dir_info.set("DataPt", pl::sref(buff.clone()));
            dir_info.set("DirStart", n(8));
            dir_info.set("DirName", pl::s("LigoTrailer"));
            pl::call(it, "Image::ExifTool::LigoGPS::ProcessLigoGPS", vec![et.clone(), dir_info, tbl])?;
        }
    }

    ret(n(1))
}

pub fn register(it: &mut Interp) {
    it.define_native("Image::ExifTool::M2TS::ParseAC3Audio", parse_ac3_audio);
    it.define_native("Image::ExifTool::M2TS::ParseAC3Descriptor", parse_ac3_descriptor);
    it.define_native("Image::ExifTool::M2TS::ParsePID", parse_pid);
    it.define_native("Image::ExifTool::M2TS::ProcessM2TS", process_m2ts);
}
