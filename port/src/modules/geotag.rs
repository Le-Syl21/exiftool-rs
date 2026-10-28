//! Image::ExifTool::Geotag (Image/ExifTool/Geotag.pm), ported sub by sub (see docs/port/PORTING.md).
//!
//! Geotagging utility routines: load GPS track logs, interpolate the track
//! at a time, and set the new GPS values.

use crate::interp::eval::ER;
use crate::interp::{Interp, Want};
use crate::modules::random_access::perl_eval1;
use crate::modules::shift::{at, localtime_now, match_g, or0, rx, time_list, vshift};
use crate::modules::write_exif::seq;
use crate::modules::write_exif::sort_num;
use crate::perl::Value;
use crate::pl::{self, arg, cap, re, ret, Caps};
use crate::runtime;

const MODULE: &str = "Image::ExifTool::Geotag";

fn n(i: i64) -> Value {
    Value::Int(i)
}

// ---- local helpers ----

fn lh(it: &mut Interp, name: &str) -> Value {
    runtime::lex_hash(it, MODULE, name)
}

fn hget(h: &Value, k: &Value) -> Value {
    h.get(&*k.to_bytes())
}

fn hset(h: &Value, k: &Value, v: Value) {
    h.set(&*k.to_bytes(), v);
}

fn hdel(h: &Value, k: &Value) -> Value {
    h.delete(&*k.to_bytes())
}

fn s(x: &str) -> Value {
    pl::s(x)
}

fn cat(items: &[Value]) -> Value {
    let mut v = Vec::new();
    for i in items {
        v.extend_from_slice(&i.to_bytes());
    }
    Value::bytes(v)
}

/// `$et->Options(NAME)`
fn opt(it: &mut Interp, et: &Value, name: &str) -> ER<Value> {
    pl::method(it, et, "Options", vec![s(name)])
}

/// `$obj->Method(ARGS)` in void context (a statement).
fn meth_void(it: &mut Interp, obj: &Value, name: &str, args: Vec<Value>) -> ER<()> {
    let mut a = vec![obj.clone()];
    a.extend(args);
    it.call_method(obj, name, a, Want::Void)?;
    Ok(())
}

/// `Time::Local::timegm(LIST)`
fn timegm(it: &mut Interp, args: Vec<Value>) -> ER<Value> {
    pl::call(it, "Time::Local::timegm", args)
}

/// `chomp` with the current `$/`.
fn chomp(it: &mut Interp, v: &mut Value) {
    let rs = it.get_scalar("main::/");
    if v.is_undef() || rs.is_undef() {
        return;
    }
    let b = v.to_bytes();
    let sep = rs.to_bytes();
    if sep.is_empty() {
        let mut e = b.len();
        while e > 0 && b[e - 1] == b'\n' {
            e -= 1;
        }
        *v = Value::bytes(&b[..e]);
    } else if b.ends_with(&sep) {
        *v = Value::bytes(&b[..b.len() - sep.len()]);
    }
}

/// `$x =~ /PAT/FLAGS`: the captures.
fn m(pat: &str, flags: &str, x: &Value) -> Option<Caps> {
    re(pat, flags).m(x)
}

/// `$x =~ s/PAT//` (first match removed): true when it matched.
fn s_del(pat: &str, x: &mut Value) -> bool {
    re(pat, "").subst(x, false, |_| Vec::new()) > 0
}

/// `-f $file`
fn is_file(v: &Value) -> bool {
    std::path::Path::new(&v.to_text()).is_file()
}

/// `\*NAME` in this package.
fn glob_ref(it: &mut Interp, name: &str) -> ER<Value> {
    perl_eval1(it, &format!("\\*{name}"), MODULE, &[])
}

/// `print $out STRING`
fn print_out(it: &mut Interp, out: &Value, v: Value) -> ER<()> {
    pl::print_to(it, Some(out), &[v])?;
    Ok(())
}

/// A hash flattened into a call's arguments (`%opts`), keeping the order
/// keys were first set.
#[derive(Clone, Default)]
struct Opts(Vec<(&'static str, Value)>);

impl Opts {
    fn set(&mut self, k: &'static str, v: Value) {
        match self.0.iter_mut().find(|(x, _)| *x == k) {
            Some(e) => e.1 = v,
            None => self.0.push((k, v)),
        }
    }

    fn flat(&self) -> Vec<Value> {
        self.0.iter().flat_map(|(k, v)| [s(k), v.clone()]).collect()
    }
}

/// `@r = $et->SetNewValue(TAG => VALUE, %opts)` (list context, to capture errors)
fn set_nv(it: &mut Interp, et: &Value, tag: &str, val: Value, opts: &Opts) -> ER<()> {
    let mut a = vec![s(tag), val];
    a.extend(opts.flat());
    pl::method_list(it, et, "SetNewValue", a)?;
    Ok(())
}

// ---- the subs ----

/// maximum time jitter
pub fn jitter(_it: &mut Interp, _a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    ret(n(2))
}

const JITTER: i64 = 2;

/// Split a line of CSV
/// Inputs: 0) line to split, 1) delimiter
/// Returns: list of items
pub fn split_csv(_it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    split_csv_v(&arg(&a, 0), &arg(&a, 1))
}

fn split_csv_v(line: &Value, delim: &Value) -> ER<Vec<Value>> {
    let mut toks = pl::split(&pl::re_dyn(&crate::perl::regex::quotemeta(&delim.to_bytes()), "")?, line, 0);
    let mut vals = Vec::new();
    while !toks.is_empty() {
        let mut v = vshift(&mut toks);
        s_del(r"^ +", &mut v); // remove leading spaces
        if s_del(r#"^""#, &mut v) {
            // quoted value must end in an odd number of quotes
            loop {
                let c = m(r#"("+)\s*$"#, "", &v);
                let odd = c.as_ref().is_some_and(|c| cap(c, 1).to_bytes().len() & 1 == 1);
                if c.is_some() && odd {
                    break;
                }
                if toks.is_empty() {
                    break;
                }
                v = cat(&[v, delim.clone(), vshift(&mut toks)]);
            }
            s_del(r#""\s*$"#, &mut v); // remove trailing quote and whitespace
            re(r#""""#, "").subst(&mut v, true, |_| b"\"".to_vec()); // un-escape quotes
        }
        vals.push(v);
    }
    Ok(vals)
}

/// Load GPS track log file or read GPS from image/video file
/// Inputs: 0) ExifTool ref, 1) track log data or file name
/// Returns: geotag hash data reference or error string
/// (see the Perl source for the members of the geotag and fix hashes)
pub fn load_track_log(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    // local ($_, $/, *EXIFTOOL_TRKFILE): $_ is a local variable here
    let r = pl::local_global(it, b'$', "main::/", |it| load_track_log_body(it, arg(&a, 0), arg(&a, 1)))?;
    ret(r)
}

fn load_track_log_body(it: &mut Interp, et: Value, val: Value) -> ER<Value> {
    let mut line = Value::Undef; // $_
    let (mut raf, mut from, mut time, mut is_date, mut no_date, mut no_date_changed, mut last_date, mut date_flarm) = (Value::Undef, Value::Undef, Value::Undef, false, Value::Undef, false, Value::Undef, Value::Undef);
    let (mut nmea_start, mut fix_secs, mut fix_times, mut last_fix): (Value, Value, Vec<Value>, Value) = (Value::Undef, Value::Undef, Vec::new(), Value::Undef);
    let nmea_seen = Value::new_hash(); // %nmea
    let mut csv_headings: Vec<Value> = Vec::new();
    let mut sort_fixes = false;
    let (mut can_cut, mut cut_pdop, mut cut_hdop, mut cut_sats, mut e0, mut e1) = (false, n(0), n(0), n(0), false, false);
    let (mut track_file, mut track_time) = (Value::Undef, Value::Undef);
    let (mut scale_speed, mut start_time) = (Value::Undef, Value::Undef);
    let user_tag = lh(it, "userTag");
    let xml_tag = lh(it, "xmlTag");
    let key_category = lh(it, "keyCategory");
    let other_conv = lh(it, "otherConv");
    let sep_tags = lh(it, "sepTags");
    let sec_per_day = runtime::lex_scalar(it, MODULE, "secPerDay").borrow().clone();

    if pl::eval_block(it, |it| runtime::require(it, "Time::Local"))?.is_none() {
        return Ok(s("Geotag feature requires Time::Local installed"));
    }
    init_user_tags_v(it, &et)?;

    // add data to existing track
    let mut geotag = pl::method(it, &et, "GetNewValue", vec![s("Geotag")])?;
    if !geotag.truthy() {
        geotag = Value::new_hash();
    }

    // initialize track points lookup
    let mut points = geotag.get("Points");
    if !points.truthy() {
        points = Value::new_hash();
        geotag.set("Points", points.clone());
    }

    // get lookup for available information types
    let mut has = geotag.get("Has");
    if !has.truthy() {
        has = Value::new_hash();
        has.set("pos", n(1));
        geotag.set("Has", has.clone());
    }

    let mut format = String::new();
    let rs = |it: &mut Interp, v: Value| it.set_scalar("main::/", v);
    // is $val track log data?
    if m(r"^(\xef\xbb\xbf)?<(\?xml|gpx)[\s>]", "", &val).is_some() {
        format = "XML".into();
        rs(it, s(">")); // set input record separator to '>' for XML/GPX data
    } else if let Some(c) = m(r"(\x0d\x0a|\x0d|\x0a)", "", &val) {
        rs(it, cap(&c, 1));
    } else {
        // $val is track file name
        let fh = glob_ref(it, "EXIFTOOL_TRKFILE")?;
        if pl::method(it, &et, "Open", vec![fh.clone(), val.clone()])?.truthy() {
            track_file = val.clone();
            raf = pl::method(it, &s("File::RandomAccess"), "new", vec![fh.clone()])?;
            let mut b = [line.clone(), n(256)];
            let r = pl::method_mut(it, &raf, "Read", &mut b, Want::Scalar)?.pop().unwrap_or_default();
            line = b[0].clone();
            if !r.truthy() {
                perl_eval1(it, "close EXIFTOOL_TRKFILE", MODULE, &[])?;
                return Ok(s(&format!("Empty track file '{}'", val.to_text())));
            }
            // look for XML or GPX header (might as well allow UTF-8 BOM)
            if m(r"^(\xef\xbb\xbf)?<(\?xml|gpx)[\s>]", "", &line).is_some() {
                format = "XML".into();
                rs(it, s(">")); // set input record separator to '>' for XML/GPX data
            } else if let Some(c) = m(r"(\x0d\x0a|\x0d|\x0a)", "", &line) {
                rs(it, cap(&c, 1));
            } else {
                perl_eval1(it, "close EXIFTOOL_TRKFILE", MODULE, &[])?;
                return Ok(s(&format!("Invalid track file '{}'", val.to_text())));
            }
            pl::method(it, &raf, "Seek", vec![n(0), n(0)])?;
            from = s(&format!("file '{}'", val.to_text()));
        } else if pl::str_eq(&val, "DATETIMEONLY") {
            geotag.set("DateTimeOnly", n(1));
            geotag.set("IsDate", n(1));
            pl::method(it, &et, "VPrint", vec![n(0), s("Geotagging date/time only")])?;
            return Ok(geotag);
        } else {
            return Ok(s(&format!("Error opening GPS file '{}'", val.to_text())));
        }
    }
    if !from.truthy() {
        // set up RAF for reading log file in memory
        raf = pl::method(it, &s("File::RandomAccess"), "new", vec![pl::sref(val.clone())])?;
        from = s("data");
    }

    // initialize cuts
    let max_hdop = opt(it, &et, "GeoMaxHDOP")?;
    let max_pdop = opt(it, &et, "GeoMaxPDOP")?;
    let min_sats = opt(it, &et, "GeoMinSats")?;
    let is_cut = max_hdop.truthy() || max_pdop.truthy() || min_sats.truthy();

    let mut num_points = n(0);
    let mut skipped = 0i64;
    let mut last_secs = n(0);
    let mut fix = Value::new_hash();
    let mut csv_delim = opt(it, &et, "CSVDelim")?;
    if csv_delim.is_undef() {
        csv_delim = s(",");
    }
    let (mut save_fix, mut save_time, mut time_span): (Vec<Value>, Vec<Value>, Value) = (Vec::new(), Vec::new(), Value::Undef);
    let set_has_cat = |has: &Value, fix: &Value, tag: &Value| {
        let kc = hget(&key_category, tag);
        if kc.truthy() {
            hset(has, &kc, n(1));
        } else if pl::str_eq(tag, "alt") {
            // validate altitude
            let alt = fix.get("alt");
            if !alt.is_undef() && !re(r"^[+-]?\d+\.?\d*", "").is_match(&alt) {
                fix.set("alt", Value::Undef);
            }
            if fix.get("alt").truthy() {
                has.set("alt", n(1)); // set "has altitude" flag if appropriate
            }
        } else if pl::str_eq(tag, "atemp") || pl::str_eq(tag, "speed") || pl::str_eq(tag, "err") {
            hset(has, tag, n(1));
        }
    };
    loop {
        {
            let mut b = [line.clone()];
            let r = pl::method_mut(it, &raf, "ReadLine", &mut b, Want::Scalar)?.pop().unwrap_or_default();
            line = b[0].clone();
            if !r.truthy() {
                break;
            }
        }
        // determine file format
        if format.is_empty() {
            s_del(r"^\xef\xbb\xbf", &mut line); // remove leading BOM if it exists
            if m(r"^(\xff\xfe|\xfe\xff)", "", &line).is_some() {
                return Ok(s("ExifTool doesn't yet read UTF16-format track logs"));
            }
            if m(r"^<(\?xml|gpx)[\s>]", "", &line).is_some() {
                // look for XML or GPX header
                format = "XML".into();
            // check for NMEA sentence
            // (must ONLY start with ones that have timestamps! eg. not GSA or PTNTHPR!)
            } else if let Some(c) = m(r"^.*\$([A-Z]{2}(RMC|GGA|GLL|ZDA)|PMGNTRK),", "", &line) {
                format = "NMEA".into();
                nmea_start = if cap(&c, 2).truthy() { cap(&c, 2) } else { cap(&c, 1) }; // save type of first sentence
            } else if m(r"^A(FLA|XSY|FIL)", "", &line).is_some() {
                // (don't set format yet because we want to read HFDTE first)
                nmea_start = s("B");
                continue;
            } else if let Some(c) = m(r"^HFDTE(?:DATE:)?(\d{2})(\d{2})(\d{2})", "", &line) {
                let y = cap(&c, 3);
                let year = pl::add(&y, &n(if pl::ge(&y, &n(70)) { 1900 } else { 2000 }));
                date_flarm = timegm(it, vec![n(0), n(0), n(0), cap(&c, 1), pl::sub(&cap(&c, 2), &n(1)), year])?;
                nmea_start = s("B");
                format = "IGC".into();
                continue;
            } else if nmea_start.truthy() && m(r"^B", "", &line).is_some() {
                // parse IGC fixes without a date
                format = "IGC".into();
            } else if m(r"^TP,D,", "", &line).is_some() {
                format = "Winplus".into();
            } else if m(r"^\s*\d+\s+.*\sypr\s*$", "", &line).is_some() && pl::split_ws(&line).len() == 12 {
                format = "Bramor".into();
            } else if ((m(r"\b(GPS)?Date", "i", &line).is_some() && m(r"\b(GPS)?(Date)?Time", "i", &line).is_some()) || m(r"\bTime\(seconds\)", "i", &line).is_some()) && pl::re_dyn(&crate::perl::regex::quotemeta(&csv_delim.to_bytes()), "")?.is_match(&line) {
                chomp(it, &mut line);
                csv_headings = split_csv_v(&line, &csv_delim)?;
                let is_columbus = csv_headings.first().is_some_and(|h| h.truthy() && pl::str_eq(h, "INDEX")); // (Columbus GPS logger)
                format = "CSV".into();
                // convert recognized headings to our parameter names
                for hd in csv_headings.iter_mut() {
                    let head = hd.clone();
                    let mut param = Value::Undef;
                    let mut xtra = s("");
                    s_del(r"^GPS ?", hd); // remove leading "GPS" to simplify regex patterns
                    let h = hd.clone();
                    let mi = |p: &str| m(p, "i", &h).is_some();
                    if mi(r"^Time ?\(seconds\)$") {
                        // DJI
                        // DJI CSV log files have a column "Time(seconds)" which is seconds since
                        // the start of the flight.  The date/time is obtained from the file name.
                        param = s("runtime");
                        let c = if track_file.truthy() { m(r"(\d{4})-(\d{2})-(\d{2})[^\/]+(\d{2})-(\d{2})-(\d{2})[^\/]*$", "", &track_file) } else { None };
                        if let Some(c) = c {
                            track_time = pl::call(it, "Image::ExifTool::TimeLocal", vec![cap(&c, 6), cap(&c, 5), cap(&c, 4), cap(&c, 3), pl::sub(&cap(&c, 2), &n(1)), cap(&c, 1)])?;
                            let utc = print_fix_time_v(it, &track_time)?;
                            let tm = Value::new_array(vec![cap(&c, 6), cap(&c, 5), cap(&c, 4), cap(&c, 3), pl::sub(&cap(&c, 2), &n(1)), pl::sub(&cap(&c, 1), &n(1900))]);
                            let tzs = pl::call(it, "Image::ExifTool::TimeZoneString", vec![tm, track_time.clone()])?;
                            pl::method(it, &et, "VPrint", vec![n(2), s(&format!("  DJI start time:  {} (local timezone is {})\n", utc.to_text(), tzs.to_text()))])?;
                        } else {
                            return Ok(s("Error getting start time from file name for DJI CSV track file"));
                        }
                    } else if mi(r"^Date ?Time") {
                        // ExifTool addition
                        param = s("datetime");
                    } else if mi(r"^Date") {
                        param = s("date");
                    } else if mi(r"^Time(?! ?\(text\))") {
                        // (ignore DJI "Time(text)" column)
                        param = s("time");
                    } else if mi(r"^(Pos)?Lat") {
                        param = s("lat");
                        if mi(r"ref$") {
                            param = s("latref");
                        }
                    } else if mi(r"^(Pos)?Lon") {
                        param = s("lon");
                        if mi(r"ref$") {
                            param = s("lonref");
                        }
                    } else if mi(r"^(Pos)?(Alt|Height)") {
                        param = s("alt");
                    } else if mi(r"^Speed") {
                        param = s("speed");
                        // (recognize units in brackets)
                        if let Some(c) = m(r"\((mph|km/h|m/s)\)", "", &h) {
                            scale_speed = hget(&other_conv, &cap(&c, 1));
                            xtra = s(&format!(" in {}", cap(&c, 1).to_text()));
                        } else if is_columbus {
                            // (Columbus GPS logger)
                            scale_speed = other_conv.get("km/h");
                            xtra = s(" in km/h");
                        } else {
                            xtra = s(" in knots");
                        }
                    } else if mi(r"^(Angle)?(Heading|Track|Bearing)") {
                        param = s("track");
                    } else if mi(r"^(Angle)?Pitch") || mi(r"^Camera ?Elevation ?Angle") {
                        param = s("pitch");
                    } else if mi(r"^(Angle)?Roll") {
                        param = s("roll");
                    } else if mi(r"^Img ?Dir") {
                        param = s("dir");
                    } else if hget(&user_tag, &pl::lc(&h)).truthy() {
                        param = hget(&user_tag, &pl::lc(&h));
                    }
                    if param.truthy() {
                        pl::method(it, &et, "VPrint", vec![n(2), cat(&[s("CSV column '"), head, s("' is "), param.clone(), xtra, s("\n")])])?;
                        *hd = param;
                    } else {
                        pl::method(it, &et, "VPrint", vec![n(2), cat(&[s("CSV column '"), head, s("' ignored\n")])])?;
                        *hd = s(""); // ignore this column
                    }
                }
                continue;
            } else if m(r#""(timelineObjects|placeVisit|activitySegment|latitudeE7)"\s*:"#, "", &line).is_some() {
                // Google Takeout JSON format
                format = "JSON".into();
                sort_fixes = true; // (fixes are not all in order for this format)
            } else if m(r#""(durationMinutesOffsetFromStartTime|startTime)"\s*:"#, "", &line).is_some() {
                format = "JSON".into(); // new Google Takeout JSON format (fixes seem to be in order)
                pl::method(it, &raf, "Seek", vec![n(0), n(0)])?; // rewind to start of file
            } else if line.to_bytes().contains(&0) {
                break; // (could be an image file)
            } else {
                // search only first 50 lines of file for a valid fix
                skipped += 1;
                if skipped > 50 {
                    break;
                }
                continue;
            }
        }
        let mut done_fix = false;
        //
        // XML format (GPX, KML, Garmin XML/TCX etc)
        //
        if format == "XML" {
            let mut args: Vec<Value> = Vec::new();
            let mut td = false;
            if m(r"^([^<]+<\/[^>]+>)", "", &line).is_some() {
                // handle simple property
                re(r"^\s+<", "").subst(&mut line, false, |_| b"<".to_vec()); // remove whitespace if only whitespace before "<"
                // Workaround for KML generated by Google Location History:
                // lat/lon/alt are space-separated; we want commas.
                re(r"(\S+)\s+(\S+)\s+(\S+)(</gx:coord>)", "").subst(&mut line, false, |c| cat(&[cap(c, 1), s(","), cap(c, 2), s(","), cap(c, 3), cap(c, 4)]).to_bytes().to_vec());
                args.push(line.clone());
            } else {
                // handle property with attributes
                re(r#"\s*=\s*(['"])\s*"#, "").subst(&mut line, true, |c| cat(&[s("="), cap(c, 1)]).to_bytes().to_vec()); // remove unnecessary white space in attributes
                args.extend(pl::split_ws(&line));
            }
            let attr_re = rx(r#"^(\w+:)?(\w+)=(['"])(.*?)\3"#, "");
            let elem_re = rx(r"([^<>]*)<(/)?(\w+:)?(\w+)(>|$)", "");
            for arg_ in &args {
                let mut pos = 0usize;
                // parse attributes (eg. GPX 'lat' and 'lon')
                // (note: ignore namespace prefixes if they exist)
                if let Some((c, p)) = match_g(&attr_re, arg_, 0) {
                    pos = p;
                    let mut tag = hget(&xml_tag, &pl::lc(&cap(&c, 2)));
                    if tag.is_undef() {
                        tag = hget(&user_tag, &pl::lc(&cap(&c, 2)));
                    }
                    if tag.truthy() {
                        hset(&fix, &tag, cap(&c, 4));
                        set_has_cat(&has, &fix, &tag);
                    }
                }
                // loop through XML elements
                while let Some((c, p)) = match_g(&elem_re, arg_, pos) {
                    pos = p;
                    let tok = pl::lc(&cap(&c, 4));
                    let mut tag = hget(&xml_tag, &tok);
                    if tag.is_undef() {
                        tag = hget(&user_tag, &tok);
                    }
                    // parse as a simple property if this element has a value
                    if !tag.is_undef() && !tag.truthy() {
                        // a containing property was opened or closed
                        if !cap(&c, 2).truthy() {
                            // opened: start a new fix
                            fix = Value::new_hash();
                            last_fix = fix.clone();
                            save_fix.clear();
                            continue;
                        } else if fix.truthy() && last_fix.truthy() && !fix.is_empty() {
                            // closed: transfer additional tags from current fix
                            for k in fix.keys() {
                                if last_fix.get(&k).is_undef() {
                                    last_fix.set(&k, fix.get(&k));
                                }
                            }
                            last_fix = Value::Undef;
                        }
                    }
                    let c1 = cap(&c, 1);
                    if !c1.to_bytes().is_empty() {
                        if tag.truthy() {
                            if pl::str_eq(&tag, "coords") {
                                // save other fixes if there are more than one
                                if !fix.get("lon").is_undef() && !fix.get("lat").is_undef() && !fix.get("alt").is_undef() {
                                    save_fix.push(Value::new_array(vec![fix.get("lon"), fix.get("lat"), fix.get("alt")]));
                                }
                                // read KML "Point" coordinates
                                let parts = pl::split(&re(",", ""), &c1, 0);
                                fix.set("lon", at(&parts, 0));
                                fix.set("lat", at(&parts, 1));
                                fix.set("alt", at(&parts, 2));
                                if fix.get("alt").truthy() {
                                    has.set("alt", n(1));
                                }
                            } else {
                                if pl::str_eq(&tok, "when") && fix.get("time").truthy() {
                                    save_time.push(c1.clone()); // flightaware KML stores times in array
                                } else {
                                    hset(&fix, &tag, c1.clone());
                                }
                                set_has_cat(&has, &fix, &tag);
                            }
                        }
                        continue;
                    } else if pl::str_eq(&tok, "td") {
                        td = true;
                    }
                    // validate and store GPS fix
                    if fix.get("lat").is_undef() || fix.get("lon").is_undef() {
                        continue;
                    }
                    if fix.get("time").is_undef() {
                        if save_time.is_empty() {
                            continue;
                        }
                        fix.set("time", vshift(&mut save_time)); // get next time in flightaware KML list
                    }
                    if !(re(r"^[+-]?\d+\.?\d*", "").is_match(&fix.get("lat")) && re(r"^[+-]?\d+\.?\d*", "").is_match(&fix.get("lon"))) {
                        if !e0 {
                            pl::method(it, &et, "VPrint", vec![n(0), cat(&[s("Coordinate format error in "), from.clone(), s("\n")])])?;
                            e0 = true;
                        }
                        continue;
                    }
                    time = get_time_v(it, &fix.get("time"))?;
                    if time.is_undef() {
                        if !e1 {
                            pl::method(it, &et, "VPrint", vec![n(0), cat(&[s("Timestamp format error in "), from.clone(), s("\n")])])?;
                            e1 = true;
                        }
                        continue;
                    }
                    is_date = true;
                    if !fix.get("pdop").is_undef() || !fix.get("hdop").is_undef() || !fix.get("nsats").is_undef() {
                        can_cut = true;
                    }
                    // generate extra fixes assuming an equally spaced track
                    if fix.get("begin").truthy() {
                        let begin = get_time_v(it, &fix.get("begin"))?;
                        fix.set("begin", Value::Undef);
                        if !begin.is_undef() && pl::lt(&begin, &time) {
                            time_span = pl::add(&or0(&time_span), &n(1));
                            fix.set("span", time_span.clone());
                            // duplicate the fix if there is only one so we will have
                            // a fix and the start and end of the TimeSpan
                            if save_fix.is_empty() {
                                save_fix.push(Value::new_array(vec![fix.get("lon"), fix.get("lat"), fix.get("alt")]));
                            }
                            let nsave = save_fix.len() as i64;
                            for i in 0..save_fix.len() {
                                let mut t = pl::add(&begin, &pl::mul(&pl::sub(&time, &begin), &pl::div(&n(i as i64), &n(nsave))?));
                                let f = Value::new_hash();
                                let sf = &save_fix[i];
                                f.set("lon", sf.at(0));
                                f.set("lat", sf.at(1));
                                f.set("alt", sf.at(2));
                                if i == 0 && hget(&points, &t).truthy() {
                                    t = pl::add(&t, &Value::Num(0.001)); // (avoid dupicates)
                                }
                                f.set("span", time_span.clone());
                                hset(&points, &t, f);
                                fix_times.push(t);
                            }
                        }
                    }
                    hset(&points, &time, fix.clone());
                    fix_times.push(time.clone()); // save times of all fixes in order
                    fix = Value::new_hash();
                    save_fix.clear();
                    num_points = pl::add(&num_points, &n(1));
                }
            }
            // last ditch check KML description for timestamp (assume it is UTC)
            if td && !fix.get("time").truthy() {
                if let Some(c) = m(r"[\s>](\d{4}-\d{2}-\d{2})[T ](\d{2}:\d{2}:\d{2}(\.\d+)?)", "", &line) {
                    fix.set("time", cat(&[cap(&c, 1), s("T"), cap(&c, 2), s("Z")]));
                }
            }
            continue;
        //
        // Winplus Beacon text file
        //
        } else if format == "Winplus" {
            // TP,D, 44.933666667, -93.186555556, 10/26/2011, 19:07:28, 0
            //       latitude      longitude      date        time
            let Some(c) = m(r"^TP,D,\s*([-+]?\d+\.\d*),\s*([-+]?\d+\.\d*),\s*(\d+)\/(\d+)\/(\d{4}),\s*(\d+):(\d+):(\d+)", "", &line) else { continue };
            fix.set("lat", cap(&c, 1));
            fix.set("lon", cap(&c, 2));
            time = timegm(it, vec![cap(&c, 8), cap(&c, 7), cap(&c, 6), cap(&c, 4), pl::sub(&cap(&c, 3), &n(1)), cap(&c, 5)])?;
            done_fix = true;
        //
        // Bramor gEO log file
        //
        } else if format == "Bramor" {
            //   1 0015   18.723675   50.672752  149 169.31 22/04/2015 07:06:55 169.31    8.88   28.07 ypr
            //   ? index  latitude    longitude  alt track  date       time     dir       pitch  roll
            let parts = pl::split_ws(&line);
            if !(parts.len() == 12 && pl::str_eq(&parts[11], "ypr")) {
                continue;
            }
            let d = pl::split(&re("/", ""), &parts[6], 0); // date (dd/mm/YYYY)
            let t = pl::split(&re(":", ""), &parts[7], 0); // time (HH:MM:SS)
            if !(d.len() == 3 && t.len() == 3) {
                continue;
            }
            for (k, i) in [("lat", 2), ("lon", 3), ("alt", 4), ("track", 5), ("dir", 8), ("pitch", 9), ("roll", 10)] {
                fix.set(k, parts[i].clone());
            }
            // (add the seconds afterwards in case some models have decimal seconds)
            time = pl::add(&timegm(it, vec![n(0), t[1].clone(), t[0].clone(), d[0].clone(), pl::sub(&d[1], &n(1)), d[2].clone()])?, &t[2]);
            // set necessary flags for extra available information
            for k in ["alt", "track", "orient"] {
                has.set(k, n(1));
            }
            done_fix = true; // save this fix
        } else if format == "CSV" {
            chomp(it, &mut line);
            let mut vals = split_csv_v(&line, &csv_delim)?;
            //
            // CSV format output of GPS/IMU POS system
            // (see the Perl source for the recognized columns)
            //
            let (mut date, mut secs) = (Value::Undef, Value::Undef);
            let neg = Value::new_hash();
            for param in &csv_headings {
                let mut val = vshift(&mut vals);
                if val.is_undef() || val.to_bytes().is_empty() {
                    break;
                }
                if !param.truthy() {
                    continue;
                }
                if pl::str_eq(param, "datetime") {
                    // (fix formats like "24.07.2016 13:47:30")
                    re(r"^(\d{2})[^\d](\d{2})[^\d](\d{4}) ", "").subst(&mut val, false, |c| cat(&[cap(c, 3), s(":"), cap(c, 2), s(":"), cap(c, 1), s(" ")]).to_bytes().to_vec());
                    let quiet = perl_eval1(it, "sub { }", MODULE, &[])?;
                    let v2 = val.clone();
                    let et2 = et.clone();
                    let r = pl::local_sig_warn(it, quiet, |it| {
                        let date_time = pl::method(it, &et2, "InverseDateTime", vec![v2.clone()])?;
                        if date_time.truthy() {
                            return Ok(Some(pl::call(it, "Image::ExifTool::GetUnixTime", vec![v2.clone(), n(2)])?));
                        }
                        Ok(None)
                    })?;
                    if let Some(d) = r {
                        date = d;
                        secs = n(0);
                    }
                } else if pl::str_eq(param, "date") {
                    if let Some(c) = m(r"^(\d{2})/(\d{2})/(\d{4})$", "", &val) {
                        date = timegm(it, vec![n(0), n(0), n(0), cap(&c, 1), pl::sub(&cap(&c, 2), &n(1)), cap(&c, 3)])?;
                    } else if let Some(c) = m(r"(\d{4}).*?(\d{2}).*?(\d{2})", "", &val) {
                        date = timegm(it, vec![n(0), n(0), n(0), cap(&c, 3), pl::sub(&cap(&c, 2), &n(1)), cap(&c, 1)])?;
                    } else if let Some(c) = m(r"^(\d{2})(\d{2})(\d{2})$", "", &val) {
                        // (Columbus GPS logger)
                        date = timegm(it, vec![n(0), n(0), n(0), cap(&c, 3), pl::sub(&cap(&c, 2), &n(1)), pl::add(&cap(&c, 1), &n(2000))])?;
                    }
                } else if pl::str_eq(param, "time") {
                    if let Some(c) = m(r"^(\d{1,2}):(\d{2}):(\d{2}(\.\d+)?).*?(([-+])(\d{1,2}):?(\d{2}))?", "", &val) {
                        secs = pl::add(&pl::mul(&pl::add(&pl::mul(&cap(&c, 1), &n(60)), &cap(&c, 2)), &n(60)), &cap(&c, 3));
                        // adjust for time zone if specified
                        if cap(&c, 5).truthy() {
                            let f = n(if pl::str_eq(&cap(&c, 6), "-") { 60 } else { -60 });
                            secs = pl::add(&secs, &pl::mul(&pl::add(&pl::mul(&cap(&c, 7), &n(60)), &cap(&c, 8)), &f));
                        }
                    } else if let Some(c) = m(r"^(\d{2})(\d{2})(\d{2})$", "", &val) {
                        // (Columbus GPS logger)
                        secs = pl::add(&pl::mul(&pl::add(&pl::mul(&cap(&c, 1), &n(60)), &cap(&c, 2)), &n(60)), &cap(&c, 3));
                    }
                } else if pl::str_eq(param, "lat") || pl::str_eq(param, "lon") {
                    let d = pl::call(it, "Image::ExifTool::GPS::ToDegrees", vec![val.clone(), n(1)])?;
                    hset(&fix, param, d);
                } else if pl::str_eq(param, "latref") {
                    if m(r"^S", "i", &val).is_some() {
                        neg.set("lat", n(1));
                    }
                } else if pl::str_eq(param, "lonref") {
                    if m(r"^W", "i", &val).is_some() {
                        neg.set("lon", n(1));
                    }
                } else if pl::str_eq(param, "runtime") {
                    date = track_time.clone();
                    secs = val.clone();
                } else if m(r"^_", "", param).is_some() {
                    hset(&fix, param, val.clone());
                } else {
                    if scale_speed.truthy() && pl::str_eq(param, "speed") {
                        val = pl::div(&val, &scale_speed)?;
                    }
                    hset(&fix, param, val.clone());
                    if hget(&sep_tags, param).truthy() {
                        hset(&has, param, n(1));
                    }
                }
            }
            // make coordinate negative according to reference direction if necessary
            for param in neg.keys() {
                if fix.get(&param).is_undef() {
                    continue;
                }
                fix.set(&param, pl::neg(&pl::abs(&fix.get(&param))));
            }
            if date.truthy() && !secs.is_undef() && !fix.get("lat").is_undef() && !fix.get("lon").is_undef() {
                time = pl::add(&date, &secs);
                if !fix.get("alt").is_undef() {
                    has.set("alt", n(1));
                }
                if !fix.get("track").is_undef() {
                    has.set("track", n(1));
                }
                if !fix.get("pitch").is_undef() {
                    has.set("orient", n(1));
                }
                done_fix = true;
            } else {
                continue;
            }
        } else if format == "JSON" {
            // Google Takeout JSON format
            if let Some(c) = m(r#""(latitudeE7|longitudeE7|latE7|lngE7|timestamp|startTime|point|durationMinutesOffsetFromStartTime|time)"\s*:\s*"?(.*?)"?,?\s*[\x0d\x0a]"#, "", &line) {
                let (k, v) = (cap(&c, 1), cap(&c, 2));
                if pl::str_eq(&k, "timestamp") || pl::str_eq(&k, "time") {
                    time = get_time_v(it, &v)?;
                    if time.truthy() && fix.get("lat").truthy() && fix.get("lon").truthy() {
                        done_fix = true;
                    }
                } else if pl::str_eq(&k, "startTime") {
                    // (new format)
                    start_time = get_time_v(it, &v)?;
                } else if pl::str_eq(&k, "latitudeE7") || pl::str_eq(&k, "latE7") {
                    fix.set("lat", pl::mul(&v, &Value::Num(1e-7)));
                } else if pl::str_eq(&k, "longitudeE7") || pl::str_eq(&k, "lngE7") {
                    fix.set("lon", pl::mul(&v, &Value::Num(1e-7)));
                } else if pl::str_eq(&k, "point") {
                    // (new format)
                    let coords = re(r"[-+]?\d+\.\d+", "").all(&v);
                    if coords.len() == 2 {
                        fix.set("lat", coords[0].clone());
                        fix.set("lon", coords[1].clone());
                    }
                } else if pl::str_eq(&k, "durationMinutesOffsetFromStartTime") && !start_time.is_undef() {
                    // (new format)
                    time = pl::add(&start_time, &pl::mul(&v, &n(60)));
                    // note: this assumes that "point" comes first, which it does in my sample
                    if time.truthy() && fix.get("lat").truthy() && fix.get("lon").truthy() {
                        done_fix = true;
                    }
                }
            }
            if !done_fix {
                continue;
            }
        }
        if done_fix {
            // DoneFix:
            is_date = true;
            hset(&points, &time, fix.clone());
            fix_times.push(time.clone());
            fix = Value::new_hash();
            num_points = pl::add(&num_points, &n(1));
            continue;
        }
        let fixh = Value::new_hash(); // %fix
        let (mut secs, mut date, mut nmea) = (Value::Undef, Value::Undef, Value::Undef);
        if format == "NMEA" {
            // ignore unrecognized NMEA sentences
            // (first 2 characters: GP=GPS, GL=GLONASS, GA=Gallileo, GN=combined, BD=Beidou)
            let Some(c) = m(r"^(.*)\$([A-Z]{2}(RMC|GGA|GLL|GSA|ZDA)|PMGNTRK|PTNTHPR),", "", &line) else { continue };
            nmea = if cap(&c, 3).truthy() { cap(&c, 3) } else { cap(&c, 2) };
            let l1 = cap(&c, 1).to_bytes().len();
            if l1 > 0 {
                line = pl::substr(&line, l1 as i64, None);
            }
        }
        let nm = |x: &str| pl::str_eq(&nmea, x);
        let lat_of = |d: &Value, mm: &Value, ns: &Value, pos: &str| -> ER<Value> { Ok(pl::mul(&pl::add(&or0(d), &pl::div(mm, &n(60))?), &n(if pl::str_eq(ns, pos) { 1 } else { -1 }))) };
        let hms = |h: &Value, mi: &Value, sc: &Value| pl::add(&pl::mul(&pl::add(&pl::mul(h, &n(60)), mi), &n(60)), sc);
        let yy = |y: &Value| pl::add(y, &n(if pl::ge(y, &n(70)) { 1900 } else { 2000 }));
        //
        // IGC (flarm) (ref 4)
        //
        if format == "IGC" {
            // B0939564531208N00557021EA007670089100207
            // BHHMMSSDDMMmmmNDDDMMmmmEAaaaaaAAAAAxxyy
            let Some(c) = m(r"^B(\d{2})(\d{2})(\d{2})(\d{2})(\d{2})(\d{3})([NS])(\d{3})(\d{2})(\d{3})([EW])([AV])(\d{5})(\d{5})", "", &line) else { continue };
            let k = |i| cap(&c, i);
            fixh.set("lat", pl::mul(&pl::add(&k(4), &pl::div(&pl::add(&k(5), &pl::div(&k(6), &n(1000))?), &n(60))?), &n(if pl::str_eq(&k(7), "N") { 1 } else { -1 })));
            fixh.set("lon", pl::mul(&pl::add(&k(8), &pl::div(&pl::add(&k(9), &pl::div(&k(10), &n(1000))?), &n(60))?), &n(if pl::str_eq(&k(11), "E") { 1 } else { -1 })));
            fixh.set("alt", if pl::str_eq(&k(12), "A") { k(14) } else { Value::Undef });
            secs = hms(&k(1), &k(2), &k(3));
            // wrap to next day if necessary
            if date_flarm.truthy() {
                if pl::lt(&secs, &pl::sub(&last_secs, &n(JITTER))) {
                    date_flarm = pl::add(&date_flarm, &sec_per_day);
                }
                date = date_flarm.clone();
            }
            nmea = s("B");
        //
        // NMEA RMC sentence (contains date)
        //
        } else if nm("RMC") {
            //  $GPRMC,hhmmss.sss,A/V,ddmm.mmmm,N/S,dddmm.mmmm,E/W,spd(knots),dir(deg),DDMMYY,,*cs
            let Some(c) = m(r"^\$[A-Z]{2}RMC,(\d{2})(\d{2})(\d+(\.\d*)?),A?,(\d*?)(\d{1,2}\.\d+),([NS]),(\d*?)(\d{1,2}\.\d+),([EW]),(\d*\.?\d*),(\d*\.?\d*),(\d{2})(\d{2})(\d+)", "", &line) else { continue };
            let k = |i| cap(&c, i);
            if pl::gt(&k(13), &n(31)) || pl::gt(&k(14), &n(12)) || pl::gt(&k(15), &n(99)) {
                continue; // validate day/month/year
            }
            fixh.set("lat", lat_of(&k(5), &k(6), &k(7), "N")?);
            fixh.set("lon", lat_of(&k(8), &k(9), &k(10), "E")?);
            if !k(11).to_bytes().is_empty() {
                fixh.set("speed", k(11));
            }
            if !k(12).to_bytes().is_empty() {
                fixh.set("track", k(12));
            }
            let year = yy(&k(15));
            secs = hms(&k(1), &k(2), &k(3));
            date = timegm(it, vec![n(0), n(0), n(0), k(13), pl::sub(&k(14), &n(1)), year])?;
        //
        // NMEA GGA sentence (no date)
        //
        } else if nm("GGA") {
            //  $GPGGA,hhmmss.sss,ddmm.mmmm,N/S,dddmm.mmmm,E/W,0=invalid,sats,hdop,alt,M,...
            let Some(c) = m(r"^\$[A-Z]{2}GGA,(\d{2})(\d{2})(\d+(\.\d*)?),(\d*?)(\d{1,2}\.\d+),([NS]),(\d*?)(\d{1,2}\.\d+),([EW]),[1-6]?,(\d+)?,(\.\d+|\d+\.?\d*)?,(-?\d+\.?\d*)?,M?", "", &line) else { continue };
            let k = |i| cap(&c, i);
            fixh.set("lat", lat_of(&k(5), &k(6), &k(7), "N")?);
            fixh.set("lon", lat_of(&k(8), &k(9), &k(10), "E")?);
            fixh.set("nsats", k(11));
            fixh.set("hdop", k(12));
            fixh.set("alt", k(13));
            secs = hms(&k(1), &k(2), &k(3));
            can_cut = true;
        //
        // NMEA GLL sentence (no date)
        //
        } else if nm("GLL") {
            //  $GPGLL,ddmm.mmmm,N/S,dddmm.mmmm,E/W,hhmmss.sss,A/V*cs
            let Some(c) = m(r"^\$[A-Z]{2}GLL,(\d*?)(\d{1,2}\.\d+),([NS]),(\d*?)(\d{1,2}\.\d+),([EW]),(\d{2})(\d{2})(\d+(\.\d*)?),A", "", &line) else { continue };
            let k = |i| cap(&c, i);
            fixh.set("lat", lat_of(&k(1), &k(2), &k(3), "N")?);
            fixh.set("lon", lat_of(&k(4), &k(5), &k(6), "E")?);
            secs = hms(&k(7), &k(8), &k(9));
        //
        // NMEA GSA sentence (satellite status, no date)
        //
        } else if nm("GSA") {
            // $GPGSA,A,3,04,05,,,,,,,,,,,pdop,hdop,vdop*HH
            let Some(c) = m(r"^\$[A-Z]{2}GSA,[AM],([23]),((?:\d*,){11}(?:\d*)),(\d+\.?\d*|\.\d+)?,(\d+\.?\d*|\.\d+)?,(\d+\.?\d*|\.\d+)?\*", "", &line) else { continue };
            let k = |i| cap(&c, i);
            fixh.set("fixtype", pl::concat(&k(1), &s("d")));
            fixh.set("sats", k(2));
            fixh.set("pdop", k(3));
            fixh.set("hdop", k(4));
            fixh.set("vdop", k(5));
            // count the number of acquired satellites
            let a_ = re(r"\d+", "").all(&fixh.get("sats"));
            fixh.set("nsats", n(a_.len() as i64));
            can_cut = true;
        //
        // NMEA ZDA sentence (date/time, contains date)
        //
        } else if nm("ZDA") {
            //  $GPZDA,hhmmss.ss,DD,MM,YYYY,tzh,tzm (hhmmss in UTC)
            let Some(c) = m(r"^\$[A-Z]{2}ZDA,(\d{2})(\d{2})(\d{2}(\.\d*)?),(\d+),(\d+),(\d+)", "", &line) else { continue };
            let k = |i| cap(&c, i);
            secs = hms(&k(1), &k(2), &k(3));
            date = timegm(it, vec![n(0), n(0), n(0), k(5), pl::sub(&k(6), &n(1)), k(7)])?;
        //
        // Magellan eXplorist PMGNTRK (Proprietary MaGellaN TRacK) sentence (optional date)
        //
        } else if nm("PMGNTRK") {
            // $PMGNTRK,ddmm.mmm,N/S,dddmm.mmm,E/W,alt,F/M,hhmmss.ss,A/V,trkname,DDMMYY*cs
            let Some(c) = m(r"^\$PMGNTRK,(\d+)(\d{2}\.\d+),([NS]),(\d+)(\d{2}\.\d+),([EW]),(-?\d+\.?\d*),([MF]),(\d{2})(\d{2})(\d+(\.\d*)?),A,(?:[^,]*,(\d{2})(\d{2})(\d+))?", "", &line) else { continue };
            let k = |i| cap(&c, i);
            fixh.set("lat", pl::mul(&pl::add(&k(1), &pl::div(&k(2), &n(60))?), &n(if pl::str_eq(&k(3), "N") { 1 } else { -1 })));
            fixh.set("lon", pl::mul(&pl::add(&k(4), &pl::div(&k(5), &n(60))?), &n(if pl::str_eq(&k(6), "E") { 1 } else { -1 })));
            fixh.set("alt", if pl::str_eq(&k(8), "M") { k(7) } else { pl::mul(&pl::mul(&k(7), &n(12)), &Value::Num(0.0254)) });
            secs = hms(&k(9), &k(10), &k(11));
            if !k(15).is_undef() {
                if pl::gt(&k(13), &n(31)) || pl::gt(&k(14), &n(12)) || pl::gt(&k(15), &n(99)) {
                    continue; // validate day/month/year
                }
                // optional date is available in PMGNTRK sentence
                let year = yy(&k(15));
                date = timegm(it, vec![n(0), n(0), n(0), k(13), pl::sub(&k(14), &n(1)), year])?;
            }
        //
        // Honeywell HMR3000 PTNTHPR (Heading Pitch Roll) sentence (no date)
        //
        } else if nm("PTNTHPR") {
            // $PTNTHPR,heading,heading status,pitch,pitch status,roll,roll status,*cs
            // (ignore this information on any alarm status)
            let Some(c) = m(r"^\$PTNTHPR,(-?[\d.]+),[MNO],(-?[\d.]+),[MNO],(-?[\d.]+),[MNO]", "", &line) else { continue };
            fixh.set("dir", cap(&c, 1));
            fixh.set("pitch", cap(&c, 2));
            fixh.set("roll", cap(&c, 3));
        } else {
            continue; // this shouldn't happen
        }
        // remember the NMEA formats we successfully read
        hset(&nmea_seen, &nmea, n(1));
        // use last date if necessary (and appropriate)
        if !secs.is_undef() && date.is_undef() && !last_date.is_undef() {
            // wrap to next day if necessary
            if pl::lt(&secs, &pl::sub(&last_secs, &n(JITTER))) {
                last_secs = pl::sub(&last_secs, &sec_per_day);
                last_date = pl::add(&last_date, &sec_per_day);
            }
            // use earlier date only if we are within 10 seconds
            if pl::lt(&pl::sub(&secs, &last_secs), &n(10)) {
                // last date is close, use it for this fix
                date = last_date.clone();
            } else {
                // last date is old, discard it
                last_date = Value::Undef;
                last_secs = Value::Undef;
            }
        }
        // save our last date/time
        if !date.is_undef() {
            last_date = date.clone();
            last_secs = secs.clone();
        }
        //
        // Add NMEA/IGC fix to our lookup
        // (this is much more complicated than it needs to be because
        //  the stupid NMEA format provides no end-of-fix indication)
        //
        // assumptions for each NMEA sentence:
        // - we only parse a time if we get a lat/lon
        // - we always get a time if we have a date
        if seq(&nmea, &nmea_start)
            || (!secs.is_undef()
                && (fix_secs.is_undef()
                    // don't combine sentences that are outside 10 seconds apart
                    || (pl::ge(&secs, &fix_secs) && pl::ge(&pl::sub(&secs, &fix_secs), &n(10)))
                    || (pl::lt(&secs, &fix_secs) && pl::ge(&pl::sub(&pl::add(&secs, &sec_per_day), &fix_secs), &n(10)))))
        {
            // start a new fix
            fix = fixh.clone();
            fix_secs = secs.clone();
            no_date_changed = false;
            // does this fix have a date/time or time stamp?
            if !date.is_undef() {
                is_date = true;
                fixh.set("isDate", n(1));
                time = pl::add(&date, &secs);
            } else if !secs.is_undef() {
                time = secs.clone();
                no_date = n(1);
                no_date_changed = true;
            } else {
                continue; // wait until we have a time before adding to lookup
            }
        } else {
            // add new data to existing fix (but don't overwrite earlier values to
            // keep the coordinates in sync with the fix time)
            for k in fixh.keys() {
                if fix.get(&k).is_undef() {
                    fix.set(&k, fixh.get(&k));
                }
            }
            if !date.is_undef() {
                if fix.get("isDate").truthy() {
                    continue;
                }
                // move this fix to the proper date
                if !fix_secs.is_undef() {
                    hdel(&points, &fix_secs);
                    if fix_times.last().is_some_and(|t| pl::num_eq(t, &fix_secs)) {
                        fix_times.pop();
                    }
                    num_points = pl::sub(&num_points, &n(1));
                    // if we wrapped to the next day since the start of this fix,
                    // we must shift the date back to the day of $fixSecs
                    if pl::lt(&secs, &fix_secs) {
                        date = pl::sub(&date, &sec_per_day);
                    }
                } else {
                    fix_secs = secs.clone();
                }
                time = pl::add(&date, &fix_secs);
                is_date = true;
                fix.set("isDate", n(1));
                // revert noDate flag if it was set for this fix
                if no_date_changed {
                    no_date = n(0);
                }
            } else if !secs.is_undef() && fix_secs.is_undef() {
                fix_secs = secs.clone();
                time = secs.clone();
                no_date = n(1);
                no_date_changed = true;
            } else {
                continue; // wait until we have a time
            }
        }
        // add fix to our lookup
        hset(&points, &time, fix.clone());
        fix_times.push(time.clone()); // save time of all fixes in order
        num_points = pl::add(&num_points, &n(1));
    }
    //
    // If the file wasn't a recognized track log, it could be one of the file types
    // supported for reading metadata, so try to extract GPS tags from it
    //
    if format.is_empty() {
        pl::method(it, &raf, "Seek", vec![n(0), n(0)])?;
        // load track from metadata in file
        let et2 = pl::method(it, &s("Image::ExifTool"), "new", vec![])?;
        let tag_key = lh(it, "tagKey");
        let tk: Vec<Value> = tag_key.keys().into_iter().map(Value::from).collect();
        let qt_utc = opt(it, &et, "QuickTimeUTC")?;
        pl::method(it, &et2, "Options", vec![s("Sort"), s("File"), s("IgnoreTags"), s("All"), s("RequestTags"), pl::join(",", &tk), s("PrintConv"), n(0), s("KeepUTCTime"), n(1), s("ExtractEmbedded"), n(3), s("QuickTimeUTC"), qt_utc])?;
        if !pl::method(it, &et2, "ExtractInfo", vec![raf.clone()])?.truthy() {
            pl::method(it, &raf, "Close", vec![])?;
            return Ok(s(&format!("Unrecognized file format '{}'", val.to_text())));
        }
        let mut tags = pl::method_list(it, &et2, "GetTagList", vec![])?;
        let (mut fix2, mut time2) = (Value::Undef, Value::Undef);
        let (mut last_grp, mut last_tag) = (s(""), s(""));
        if !tags.is_empty() {
            let l = tags.last().unwrap().clone();
            tags.push(l); // duplicate last tag to act as an end marker
            is_date = true;
            sort_fixes = true; // (fixes may not be in order)
        }
        let speed_conv = lh(it, "speedConv");
        for tag in &tags {
            let grp = pl::method(it, &et2, "GetGroup", vec![tag.clone(), n(3)])?;
            if !seq(&grp, &last_grp) || seq(tag, &last_tag) {
                // add fix to our lookup
                if time2.truthy() && fix2.get("lat").truthy() && fix2.get("lon").truthy() {
                    let pt = hget(&points, &time2);
                    if pt.truthy() {
                        // could possibly happen if time jumps around
                        for k in fix2.keys() {
                            pt.set(&k, fix2.get(&k));
                        }
                    } else {
                        hset(&points, &time2, fix2.clone());
                        // convert speed to knots
                        if fix2.get("speed").truthy() {
                            fix2.set("speed", pl::div(&fix2.get("speed"), &speed_conv.get("K"))?);
                        }
                        fix_times.push(time2.clone()); // save time of all fixes in order
                        num_points = pl::add(&num_points, &n(1));
                    }
                }
                last_grp = grp.clone();
                time2 = Value::Undef;
                fix2 = Value::new_hash();
            }
            last_tag = tag.clone();
            let mut name = tag.clone();
            re(r" .*", "").subst(&mut name, false, |_| Vec::new());
            let key = hget(&tag_key, &name);
            if !key.truthy() {
                if key.is_undef() {
                    pl::method(it, &et, "Warn", vec![cat(&[s("Internal error with tag name "), key.clone(), s(" "), name.clone(), s(" ("), tag.clone(), s(")")])])?;
                }
                continue;
            }
            let v = pl::method(it, &et2, "GetValue", vec![tag.clone()])?;
            if pl::str_eq(&key, "time") {
                if !time2.truthy() {
                    time2 = pl::call(it, "Image::ExifTool::GetUnixTime", vec![v])?;
                }
            } else if hget(&fix2, &key).is_undef() {
                hset(&fix2, &key, v);
                let kc = hget(&key_category, &key);
                if kc.truthy() {
                    hset(&has, &kc, n(1));
                }
            }
        }
    }
    pl::method(it, &raf, "Close", vec![])?;

    // set date flags
    if no_date.truthy() && !geotag.get("NoDate").truthy() {
        if is_date {
            pl::method(it, &et, "Warn", vec![s("Fixes are date-less -- will use time-only interpolation")])?;
        } else {
            pl::method(it, &et, "Warn", vec![s("Some fixes are date-less -- may use time-only interpolation")])?;
        }
        geotag.set("NoDate", n(1));
    }
    if is_date {
        geotag.set("IsDate", n(1));
    }

    // cut bad fixes if necessary
    if is_cut && can_cut {
        (cut_pdop, cut_hdop, cut_sats) = (n(0), n(0), n(0));
        let mut good_times = Vec::new();
        for t in &fix_times {
            fix = hget(&points, t);
            if !fix.truthy() {
                continue;
            }
            if max_pdop.truthy() && fix.get("pdop").truthy() && pl::gt(&fix.get("pdop"), &max_pdop) {
                hdel(&points, t);
                cut_pdop = pl::add(&cut_pdop, &n(1));
            } else if max_hdop.truthy() && fix.get("hdop").truthy() && pl::gt(&fix.get("hdop"), &max_hdop) {
                hdel(&points, t);
                cut_hdop = pl::add(&cut_hdop, &n(1));
            } else if min_sats.truthy() && !fix.get("nsats").is_undef() && !pl::str_eq(&fix.get("nsats"), "") && pl::lt(&fix.get("nsats"), &min_sats) {
                hdel(&points, t);
                cut_sats = pl::add(&cut_sats, &n(1));
            } else {
                good_times.push(t.clone());
            }
        }
        fix_times = good_times; // update fix times
        num_points = pl::sub(&num_points, &cut_pdop);
        num_points = pl::sub(&num_points, &cut_hdop);
        num_points = pl::sub(&num_points, &cut_sats);
    }
    // sort fixes if necessary
    if sort_fixes {
        sort_num(&mut fix_times);
    }
    // mark first fix of the track
    while !fix_times.is_empty() {
        fix = hget(&points, &fix_times[0]);
        if !fix.truthy() {
            fix_times.remove(0);
            continue;
        }
        fix.set("first", n(1));
        break;
    }
    let verbose = opt(it, &et, "Verbose")?;
    if verbose.truthy() {
        let out = opt(it, &et, "TextOut")?;
        if format.is_empty() {
            format = "unknown".into();
        }
        print_out(it, &out, s(&format!("Loaded {} points from {format}-format GPS track log {}\n", num_points.to_text(), from.to_text())))?;
        if cut_pdop.truthy() {
            print_out(it, &out, s(&format!("Ignored {} points due to GeoMaxPDOP cut\n", cut_pdop.to_text())))?;
        }
        if cut_hdop.truthy() {
            print_out(it, &out, s(&format!("Ignored {} points due to GeoMaxHDOP cut\n", cut_hdop.to_text())))?;
        }
        if cut_sats.truthy() {
            print_out(it, &out, s(&format!("Ignored {} points due to GeoMinSats cut\n", cut_sats.to_text())))?;
        }
        if num_points.truthy() && pl::gt(&verbose, &n(1)) {
            let mut lbl = ["start:", "end:  "];
            // (fixes may be in reverse order in GPX files)
            if pl::gt(&at(&fix_times, 0), &fix_times.last().cloned().unwrap_or_default()) {
                lbl.reverse();
            }
            let t0 = print_fix_time_v(it, &at(&fix_times, 0))?;
            print_out(it, &out, cat(&[s(&format!("  GPS track {} ", lbl[0])), t0, s("\n")]))?;
            if pl::gt(&verbose, &n(3)) {
                for t in fix_times.clone() {
                    let p = print_fix_v(it, &points, &[t])?;
                    print_out(it, &out, p)?;
                }
            }
            let t1 = print_fix_time_v(it, &fix_times.last().cloned().unwrap_or_default())?;
            print_out(it, &out, cat(&[s(&format!("  GPS track {} ", lbl[1])), t1, s("\n")]))?;
        }
    }
    if num_points.truthy() {
        // reset timestamp list to force it to be regenerated
        geotag.delete("Times");
        // set flags for available information
        if nmea_seen.get("GGA").truthy() || nmea_seen.get("PMGNTRK").truthy() || nmea_seen.get("B").truthy() {
            has.set("alt", n(1)); // alt
        }
        if nmea_seen.get("RMC").truthy() {
            has.set("track", n(1)); // track, speed
        }
        if nmea_seen.get("PTNTHPR").truthy() {
            has.set("orient", n(1)); // dir, pitch, roll
        }
        return Ok(geotag); // success!
    }
    Ok(cat(&[s("No track points found in GPS "), from]))
}

/// Get floating point UTC time
/// Inputs: 0) XML time string
/// Returns: floating point time or undef on error
pub fn get_time(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    ret(get_time_v(it, &arg(&a, 0))?)
}

fn get_time_v(it: &mut Interp, time_str: &Value) -> ER<Value> {
    let Some(c) = m(r"^(\d{4})-(\d+)-(\d+)T(\d+):(\d+):(\d+)(\.\d+)?(.*)", "", time_str) else { return Ok(Value::Undef) };
    let mut time = timegm(it, vec![cap(&c, 6), cap(&c, 5), cap(&c, 4), cap(&c, 3), pl::sub(&cap(&c, 2), &n(1)), cap(&c, 1)])?;
    if cap(&c, 7).truthy() {
        time = pl::add(&time, &cap(&c, 7)); // add fractional seconds
    }
    let tz = cap(&c, 8);
    // adjust for time zone (otherwise assume UTC)
    // - allow timezone of +-HH:MM, +-H:MM, +-HHMM or +-HH since
    //   the spec is unclear about timezone format
    if let Some(t) = m(r"^([-+])(\d+):(\d{2})\b", "", &tz).or_else(|| m(r"^([-+])(\d{2})(\d{2})?\b", "", &tz)) {
        let mut tzv = pl::mul(&pl::add(&pl::mul(&cap(&t, 2), &n(60)), &or0(&cap(&t, 3))), &n(60));
        if pl::str_eq(&cap(&t, 1), "+") {
            tzv = pl::mul(&tzv, &n(-1)); // opposite sign to change back to UTC
        }
        time = pl::add(&time, &tzv);
    }
    Ok(time)
}

/// Apply Geosync time correction
/// Inputs: 0) ExifTool ref, 1) Unix UTC time value
/// Returns: sync time difference (and updates input time), or undef if no sync
pub fn apply_sync_corr(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (sync, time) = apply_sync_corr_v(it, &arg(&a, 0), &arg(&a, 1))?;
    if let Some(t) = time {
        it.set_arg(1, t)?;
    }
    ret(sync)
}

/// ApplySyncCorr: the sync, and the corrected time when it was changed.
fn apply_sync_corr_v(it: &mut Interp, et: &Value, time: &Value) -> ER<(Value, Option<Value>)> {
    let mut sync = pl::method(it, et, "GetNewValue", vec![s("Geosync")])?;
    if pl::ref_of(&sync) == "HASH" {
        let sync_times = sync.get("Times");
        if sync_times.truthy() {
            // find the nearest 2 sync points
            let (mut i0, mut i1) = (0i64, sync_times.len() as i64 - 1);
            while i1 > i0 + 1 {
                let pt = (i0 + i1) / 2;
                if pl::lt(time, &sync_times.at(pt)) {
                    i1 = pt;
                } else {
                    i0 = pt;
                }
            }
            let (t0, t1) = (sync_times.at(i0), sync_times.at(i1));
            // interpolate/extrapolate to account for linear camera clock drift
            let sync_points = sync.get("Points");
            let f = if pl::num_eq(&t1, &t0) { n(0) } else { pl::div(&pl::sub(time, &t0), &pl::sub(&t1, &t0))? };
            sync = pl::add(&pl::mul(&hget(&sync_points, &t1), &f), &pl::mul(&hget(&sync_points, &t0), &pl::sub(&n(1), &f)));
        } else {
            sync = sync.get("Offset"); // use fixed time offset
        }
        let t = pl::add(time, &sync);
        Ok((sync, Some(t)))
    } else {
        Ok((Value::Undef, None))
    }
}

/// Scan outwards for a fix containing the requested parameter
/// Inputs: 0) name of fix parameter, 1) reference to list of fix times,
///         2) reference to fix points hash, 3) index of starting time,
///         4) direction to scan (-1 or +1), 5) maximum time difference
/// Returns: 0) time for fix containing requested information (or undef)
///          1) the corresponding fix, 2) the value of the requested fix parameter
pub fn scan_outwards(_it: &mut Interp, a: Vec<Value>, want: Want) -> ER<Vec<Value>> {
    let r = scan_outwards_v(&arg(&a, 0), &arg(&a, 1), &arg(&a, 2), arg(&a, 3).to_i64(), arg(&a, 4).to_i64(), &arg(&a, 5));
    match r {
        Some((t, p, v)) => Ok(if want == Want::List { vec![t, p, v] } else { vec![v] }),
        None => pl::ret_nothing(want),
    }
}

fn scan_outwards_v(key: &Value, times: &Value, points: &Value, mut i: i64, dir: i64, max_secs: &Value) -> Option<(Value, Value, Value)> {
    let t0 = times.at(i);
    loop {
        i += dir;
        if i < 0 || i >= times.len() as i64 {
            break;
        }
        let t = times.at(i);
        if pl::gt(&pl::abs(&pl::sub(&t, &t0)), max_secs) {
            break; // don't look too far
        }
        let p = hget(points, &t);
        let v = hget(&p, key);
        if !v.is_undef() {
            return Some((t, p, v));
        }
    }
    None
}

/// Find nearest fix containing the specified parameter
/// Inputs: 0) ExifTool ref, 1) name of fix parameter, 2) reference to list of fix times,
///         3) reference to fix points hash, 4) index of starting time,
///         5) direction to scan (-1, +1 or undef), 6) maximum time difference
/// Returns: reference to fix hash or undef
pub fn find_fix(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    ret(find_fix_v(it, &arg(&a, 0), &arg(&a, 1), &arg(&a, 2), &arg(&a, 3), &arg(&a, 4), &arg(&a, 5), &arg(&a, 6))?)
}

#[allow(clippy::too_many_arguments)]
fn find_fix_v(it: &mut Interp, et: &Value, key: &Value, times: &Value, points: &Value, i: &Value, dir: &Value, max_secs: &Value) -> ER<Value> {
    let (mut t, mut p) = (Value::Undef, Value::Undef);
    let i = i.to_i64();
    if dir.truthy() {
        if let Some((a, b, _)) = scan_outwards_v(key, times, points, i, dir.to_i64(), max_secs) {
            (t, p) = (a, b);
        }
    } else {
        let (t1, p1) = scan_outwards_v(key, times, points, i, -1, max_secs).map(|(a, b, _)| (a, b)).unwrap_or_default();
        let (t2, p2) = scan_outwards_v(key, times, points, i, 1, max_secs).map(|(a, b, _)| (a, b)).unwrap_or_default();
        if !t1.is_undef() {
            if !t2.is_undef() {
                // both surrounding points are valid, so take the closest one
                (t, p) = if pl::lt(&pl::sub(&t, &t1), &pl::sub(&t2, &t)) { (t1, p1) } else { (t2, p2) };
            } else {
                (t, p) = (t1, p1);
            }
        } else if !t2.is_undef() {
            (t, p) = (t2, p2);
        }
    }
    if !p.is_undef() && pl::gt(&et.path(&["OPTIONS", "Verbose"]), &n(2)) {
        let pf = print_fix_v(it, points, &[t])?;
        pl::method(it, et, "VPrint", vec![n(2), s(&format!("  Taking {} from fix:\n", key.to_text())), pf])?;
    }
    Ok(p)
}

/// Set new geotagging values according to date/time
/// Inputs: 0) ExifTool object ref, 1) date/time value (or undef to delete tags)
///         2) optional write group
/// Returns: error string, or '' on success
/// Notes: Uses track data stored in ExifTool NEW_VALUE for Geotag tag
pub fn set_geo_values(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, mut val, write_group) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    let mut geotag = pl::method(it, &et, "GetNewValue", vec![s("Geotag")])?;
    let verbose = opt(it, &et, "Verbose")?;
    let (mut fix, mut time, mut fsec, mut no_date, mut second_try, mut i_ext, mut i_dir) = (Value::Undef, Value::Undef, Value::Undef, false, false, Value::Undef, Value::Undef);
    let sec_per_day = runtime::lex_scalar(it, MODULE, "secPerDay").borrow().clone();
    let user_tag = lh(it, "userTag");
    let fix_info_keys = lh(it, "fixInfoKeys");
    let sep_tags = lh(it, "sepTags");
    let cyclical = lh(it, "cyclical");
    let cyc180 = lh(it, "cyc180");
    let vp = |it: &mut Interp, args: Vec<Value>| -> ER<()> {
        let mut a = vec![n(2)];
        a.extend(args);
        pl::method(it, &et, "VPrint", a)?;
        Ok(())
    };

    // remove date if none of our fixes had date information
    if val.truthy() && geotag.truthy() && !geotag.get("IsDate").truthy() {
        s_del(r"^\S+\s+", &mut val);
    }

    // maximum time (sec) from nearest GPS fix when position is still considered valid
    let mut geo_max_int_secs = opt(it, &et, "GeoMaxIntSecs")?;
    let mut geo_max_ext_secs = opt(it, &et, "GeoMaxExtSecs")?;

    // use 30 minutes for a default
    if geo_max_int_secs.is_undef() {
        geo_max_int_secs = n(1800);
    }
    if geo_max_ext_secs.is_undef() {
        geo_max_ext_secs = n(1800);
    }

    // ($$geotag{Times} autovivifies $geotag into a hash when it is undefined)
    if geotag.is_undef() {
        geotag = Value::new_hash();
    }
    let mut times = geotag.get("Times");
    let points = geotag.get("Points");
    let has = geotag.get("Has");
    let mut err = s("");
    // loop to try date/time value first, then time-only value
    while !val.is_undef() {
        if geotag.is_undef() {
            err = s("No GPS track loaded");
            break;
        }
        if !times.truthy() {
            // generate sorted timestamp list for binary search
            let mut t: Vec<Value> = points.keys().into_iter().map(Value::from).collect();
            sort_num(&mut t);
            times = Value::new_array(t);
            geotag.set("Times", times.clone());
        }
        if !((times.truthy() && !times.is_empty()) || geotag.get("DateTimeOnly").truthy()) {
            err = s("GPS track is empty");
            break;
        }
        if pl::eval_block(it, |it| runtime::require(it, "Time::Local"))?.is_none() {
            err = s("Geotag feature requires Time::Local installed");
            break;
        }
        // convert date/time to UTC
        let (year, mon, day, hr, min, sec, fs, tz, t0, t1, t2);
        if let Some(c) = m(r"^(\d{4}):(\d+):(\d+)\s+(\d+):(\d+):(\d+)(\.\d*)?(Z|([-+])(\d+):(\d+))?", "", &val) {
            // valid date/time value
            (year, mon, day, hr, min, sec, fs, tz, t0, t1, t2) = (cap(&c, 1), cap(&c, 2), cap(&c, 3), cap(&c, 4), cap(&c, 5), cap(&c, 6), cap(&c, 7), cap(&c, 8), cap(&c, 9), cap(&c, 10), cap(&c, 11));
        } else if let Some(c) = m(r"^(\d{2}):(\d+):(\d+)(\.\d*)?(Z|([-+])(\d+):(\d+))?", "", &val) {
            // valid time-only value
            (hr, min, sec, fs, tz, t0, t1, t2) = (cap(&c, 1), cap(&c, 2), cap(&c, 3), cap(&c, 4), cap(&c, 5), cap(&c, 6), cap(&c, 7), cap(&c, 8));
            // use Jan. 2 to avoid going negative after tz adjustment
            (year, mon, day) = (n(1970), n(1), n(2));
            no_date = true;
        } else {
            err = s("Invalid date/time (use YYYY:mm:dd HH:MM:SS[.ss][+/-HH:MM|Z])");
            break;
        }
        if tz.truthy() {
            time = timegm(it, vec![sec.clone(), min.clone(), hr.clone(), day.clone(), pl::sub(&mon, &n(1)), year.clone()])?;
            // use timezone from date/time value
            if !pl::str_eq(&tz, "Z") {
                let tzmin = pl::add(&pl::mul(&t1, &n(60)), &t2);
                let x = if pl::str_eq(&t0, "-") { pl::neg(&tzmin) } else { tzmin };
                time = pl::sub(&time, &pl::mul(&x, &n(60)));
            }
        } else {
            // assume local timezone
            time = pl::call(it, "Image::ExifTool::TimeLocal", vec![sec.clone(), min.clone(), hr.clone(), day.clone(), pl::sub(&mon, &n(1)), year.clone()])?;
        }
        // add fractional seconds
        if fs.truthy() && !pl::str_eq(&fs, ".") {
            time = pl::add(&time, &fs);
        }

        // bring UTC time back to Jan. 1 if no date is given
        // (don't use '%' operator here because it drops fractional seconds)
        if no_date {
            time = pl::sub(&time, &pl::mul(&pl::int(&pl::div(&time, &sec_per_day)?), &sec_per_day));
        }

        // apply time synchronization if available
        let (sync, nt) = apply_sync_corr_v(it, &et, &time)?;
        if let Some(t) = nt {
            time = t;
        }

        // save fractional seconds string
        fsec = match m(r"(\.\d+)$", "", &time) {
            Some(c) => cap(&c, 1),
            None => s(""),
        };

        if pl::gt(&opt(it, &et, "Verbose")?, &n(1)) && !second_try {
            let out = opt(it, &et, "TextOut")?;
            let mut st = s("");
            if !sync.is_undef() {
                st = pl::concat(&st, &pl::sprintf(" (incl. Geosync offset of %+.3f sec)", &[sync.clone()]));
            }
            if !tz.truthy() {
                let tm = Value::new_array(vec![sec.clone(), min.clone(), hr.clone(), day.clone(), pl::sub(&mon, &n(1)), pl::sub(&year, &n(1900))]);
                let tzs = pl::call(it, "Image::ExifTool::TimeZoneString", vec![tm, time.clone()])?;
                st = cat(&[st, s(" (local timezone is "), tzs, s(")")]);
            }
            let pft = print_fix_time_v(it, &time)?;
            print_out(it, &out, cat(&[s("  Geotime value:   "), pft, st, s("\n")]))?;
        }
        let ddto = geotag.get("DateTimeOnly").truthy();
        if !times.truthy() || times.is_empty() {
            fix = Value::new_hash(); // dummy fix to geotag date/time only
        // interpolate GPS track at $time
        } else if pl::lt(&time, &times.at(0)) {
            if pl::lt(&time, &pl::sub(&times.at(0), &geo_max_ext_secs)) {
                if !err.truthy() {
                    err = s("Time is too far before track");
                }
                if pl::gt(&verbose, &n(2)) {
                    let p = print_fix_time_v(it, &times.at(0))?;
                    vp(it, vec![s("  Track start:     "), p, s("\n")])?;
                }
                if ddto {
                    fix = Value::new_hash();
                }
            } else {
                fix = hget(&points, &times.at(0));
                (i_ext, i_dir) = (n(0), n(1));
                if pl::gt(&verbose, &n(2)) {
                    let p = print_fix_v(it, &points, &[times.at(0)])?;
                    vp(it, vec![s("  Taking pos from fix:\n"), p])?;
                }
            }
        } else if pl::gt(&time, &times.at(-1)) {
            if pl::gt(&time, &pl::add(&times.at(-1), &geo_max_ext_secs)) {
                if !err.truthy() {
                    err = s("Time is too far beyond track");
                }
                if pl::gt(&verbose, &n(2)) {
                    let p = print_fix_time_v(it, &times.at(-1))?;
                    vp(it, vec![s("  Track end:       "), p, s("\n")])?;
                }
                if ddto {
                    fix = Value::new_hash();
                }
            } else {
                fix = hget(&points, &times.at(-1));
                (i_ext, i_dir) = (n(times.len() as i64 - 1), n(-1));
                if pl::gt(&verbose, &n(2)) {
                    let p = print_fix_v(it, &points, &[times.at(-1)])?;
                    vp(it, vec![s("  Taking pos from fix:\n"), p])?;
                }
            }
        } else {
            // find nearest 2 points in time
            let (mut i0, mut i1) = (0i64, times.len() as i64 - 1);
            while i1 > i0 + 1 {
                let pt = (i0 + i1) / 2;
                if pl::lt(&time, &times.at(pt)) {
                    i1 = pt;
                } else {
                    i0 = pt;
                }
            }
            // do linear interpolation for position
            let t0 = times.at(i0);
            let t1 = times.at(i1);
            let p1 = hget(&points, &t1);
            // check to see if we are extrapolating before the first entry in a track
            let max_secs = if p1.get("first").truthy() && geo_max_int_secs.truthy() { geo_max_ext_secs.clone() } else { geo_max_int_secs.clone() };
            let tn; // find time of nearest fix
            if pl::lt(&pl::sub(&time, &t0), &pl::sub(&t1, &time)) {
                tn = t0.clone();
                i_ext = n(i0);
            } else {
                tn = t1.clone();
                i_ext = n(i1);
            }
            // don't interpolate if fixes are too far apart
            // (but always interpolate fixes inside the same TimeSpan)
            let sp0 = hget(&points, &t0).get("span");
            if pl::gt(&pl::sub(&t1, &t0), &max_secs) && (!p1.get("span").truthy() || !sp0.truthy() || !pl::num_eq(&p1.get("span"), &sp0)) {
                // treat as an extrapolation -- use nearest fix if close enough
                if pl::gt(&pl::abs(&pl::sub(&time, &tn)), &geo_max_ext_secs) {
                    if !err.truthy() {
                        err = s("Time is too far from nearest GPS fix");
                    }
                    if pl::gt(&verbose, &n(2)) {
                        let p = print_fix_time_v(it, &tn)?;
                        vp(it, vec![s("  Nearest fix:     "), p, s(" ("), pl::int(&pl::abs(&pl::sub(&time, &tn))), s(" sec away)\n")])?;
                    }
                    if ddto {
                        fix = Value::new_hash();
                    }
                } else {
                    fix = hget(&points, &tn);
                    if pl::gt(&verbose, &n(2)) {
                        let p = print_fix_v(it, &points, &[tn.clone()])?;
                        vp(it, vec![s("  Taking pos from fix:\n"), p])?;
                    }
                }
            } else {
                let f0 = if pl::num_eq(&t1, &t0) { n(0) } else { pl::div(&pl::sub(&time, &t0), &pl::sub(&t1, &t0))? };
                let p0 = hget(&points, &t0);
                if pl::gt(&verbose, &n(2)) {
                    let p = print_fix_v(it, &points, &[t0.clone(), t1.clone()])?;
                    vp(it, vec![cat(&[s("  Interpolating between fixes (f="), f0.clone(), s("):\n")]), p])?;
                }
                fix = Value::new_hash();
                // copy user-defined tags from nearest fix
                let ptn = hget(&points, &tn);
                for k in user_tag.keys() {
                    let ut = user_tag.get(&k);
                    hset(&fix, &ut, hget(&ptn, &ut));
                }
                // loop through available fix information categories
                // (pos, track, alt, orient)
                'category: for category in ["pos", "track", "alt", "orient", "atemp", "err", "dop"] {
                    if !has.get(category).truthy() {
                        continue;
                    }
                    let mut f: Value;
                    let (mut p0b, mut p1b, mut f0b) = (Value::Undef, Value::Undef, Value::Undef);
                    // loop through specific fix information keys
                    // (lat, lon, alt, track, speed, dir, pitch, roll)
                    for key in fix_info_keys.get(category).items() {
                        let mut v0 = hget(&p0, &key);
                        let mut v1 = hget(&p1, &key);
                        if !v0.is_undef() && !v1.is_undef() {
                            f = f0.clone();
                        } else if !f0b.is_undef() {
                            v0 = hget(&p0b, &key);
                            v1 = hget(&p1b, &key);
                            if v0.is_undef() || v1.is_undef() {
                                continue;
                            }
                            f = f0b.clone();
                        } else {
                            if hget(&sep_tags, &key).truthy() {
                                continue; // (don't scan outwards for some formats, eg. CSV)
                            }
                            // scan outwards looking for fixes with the required information
                            // (NOTE: SHOULD EVENTUALLY DO THIS FOR EXTRAPOLATION TOO!)
                            let (t0b, t1b);
                            if !v0.is_undef() {
                                t0b = t0.clone();
                                p0b = p0.clone();
                            } else {
                                let r = scan_outwards_v(&key, &times, &points, i0, -1, &max_secs);
                                let Some((a_, b_, c_)) = r else { continue 'category };
                                (t0b, p0b, v0) = (a_, b_, c_);
                            }
                            if !v1.is_undef() {
                                t1b = t1.clone();
                                p1b = p1.clone();
                            } else {
                                let r = scan_outwards_v(&key, &times, &points, i1, 1, &max_secs);
                                let Some((a_, b_, c_)) = r else { continue 'category };
                                (t1b, p1b, v1) = (a_, b_, c_);
                            }
                            // re-calculate the interpolation factor
                            f0b = if pl::num_eq(&t1b, &t0b) { n(0) } else { pl::div(&pl::sub(&time, &t0b), &pl::sub(&t1b, &t0b))? };
                            f = f0b.clone();
                            if pl::gt(&verbose, &n(2)) {
                                let p = print_fix_v(it, &points, &[t0b.clone(), t1b.clone()])?;
                                vp(it, vec![cat(&[s(&format!("  Interpolating {category} between fixes (f=")), f.clone(), s("):\n")]), p])?;
                            }
                        }
                        // must interpolate cyclical values differently
                        if hget(&cyclical, &key).truthy() && pl::gt(&pl::abs(&pl::sub(&v1, &v0)), &n(180)) {
                            // the acute angle spans the discontinuity, so add
                            // 360 degrees to the smaller angle before interpolating
                            // ($v0 < $v1 ? $v0 += 360 : $v1 += 360 parses as
                            //  ($v0 < $v1 ? ($v0 += 360) : $v1) += 360 in perl)
                            if pl::lt(&v0, &v1) {
                                v0 = pl::add(&pl::add(&v0, &n(360)), &n(360));
                            } else {
                                v1 = pl::add(&v1, &n(360));
                            }
                            hset(&fix, &key, pl::add(&pl::mul(&v1, &f), &pl::mul(&v0, &pl::sub(&n(1), &f))));
                            // some ranges are -180 to 180, others are 0 to 360
                            let max = n(if hget(&cyc180, &key).truthy() { 180 } else { 360 });
                            if pl::ge(&hget(&fix, &key), &max) {
                                hset(&fix, &key, pl::sub(&hget(&fix, &key), &n(360)));
                            }
                        } else {
                            // simple linear interpolation
                            hset(&fix, &key, pl::add(&pl::mul(&v1, &f), &pl::mul(&v0, &pl::sub(&n(1), &f))));
                        }
                    }
                }
            }
        }
        if fix.truthy() {
            err = s(""); // success!
        } else if geotag.get("NoDate").truthy() && !no_date && s_del(r"^\S+\s+", &mut val) {
            // try again with no date since some of our track points are date-less
            second_try = true;
            continue;
        }
        break;
    }
    if fix.truthy() {
        let (mut gps_date, mut gps_alt, mut gps_alt_ref) = (Value::Undef, Value::Undef, Value::Undef);
        let t = time_list(it, &pl::int(&time), false);
        let gps_time = pl::concat(&pl::sprintf("%.2d:%.2d:%.2d", &[t[2].clone(), t[1].clone(), t[0].clone()]), &fsec);
        // write GPSDateStamp if date included in track log, otherwise delete it
        if !no_date {
            gps_date = pl::sprintf("%.2d:%.2d:%.2d", &[pl::add(&t[5], &n(1900)), pl::add(&t[4], &n(1)), t[3].clone()]);
        }
        // write GPSAltitude tags if altitude included in track log, otherwise delete them
        let mut alt = fix.get("alt");
        if alt.is_undef() && has.get("alt").truthy() && !i_ext.is_undef() {
            let t_fix = find_fix_v(it, &et, &s("alt"), &times, &points, &i_ext, &i_dir, &geo_max_ext_secs)?;
            if t_fix.truthy() {
                alt = t_fix.get("alt");
            }
        }
        // set new GPS tag values (EXIF, or XMP if write group is 'xmp')
        let (mut xmp, mut exif, mut qt) = (false, false, false);
        let mut opts = Opts::default();
        opts.set("Type", s("ValueConv")); // write ValueConv values
        if write_group.truthy() {
            opts.set("Group", write_group.clone());
            xmp = m(r"xmp", "i", &write_group).is_some();
            exif = m(r"^(exif|gps)$", "i", &write_group).is_some();
            qt = m(r"^(quicktime|keys|itemlist|userdata)$", "i", &write_group).is_some();
        }
        // set QuickTime GPSCoordinates
        let mut coords = cat(&[fix.get("lat"), s(" "), fix.get("lon")]);
        if !alt.is_undef() {
            gps_alt = pl::abs(&alt);
            gps_alt_ref = n(if pl::lt(&alt, &n(0)) { 1 } else { 0 });
            coords = cat(&[coords, s(" "), alt.clone()]);
        }
        set_nv(it, &et, "GPSCoordinates", coords, &opts)?;
        // also Geolocate if specified
        let nv_hash = pl::sref(Value::Undef);
        let geoloc = pl::method(it, &et, "GetNewValue", vec![s("Geolocate"), nv_hash.clone()])?;
        let nv_hash = nv_hash.deref();
        if geoloc.truthy() && m(r"\bgeotag\b", "i", &geoloc).is_some() {
            let wg = nv_hash.get("WantGroup");
            let tag = if wg.truthy() { cat(&[wg, s(":"), s("Geolocate")]) } else { s("Geolocate") };
            // pass along any regular expressions to qualify geolocation search
            let parts: Vec<Value> = pl::split(&re(r"\s*,\s*", ""), &geoloc, 0).into_iter().filter(|p| p.to_bytes().contains(&b'/')).collect();
            let mut parms = pl::join(",", &parts);
            if parms.truthy() {
                parms = cat(&[s(","), parms, s(",both")]);
            }
            meth_void(it, &et, "SetNewValue", vec![tag, cat(&[fix.get("lat"), s(","), fix.get("lon"), parms])])?;
            // (the Geolocate tag will be restored to its original value
            // by RestoreNewValues before the next file in batch processing)
        }
        if qt {
            return ret(err); // all done if writing to QuickTime only
        }
        // (capture error messages by calling SetNewValue in list context)
        set_nv(it, &et, "GPSLatitude", fix.get("lat"), &opts)?;
        set_nv(it, &et, "GPSLongitude", fix.get("lon"), &opts)?;
        set_nv(it, &et, "GPSAltitude", gps_alt, &opts)?;
        set_nv(it, &et, "GPSAltitudeRef", gps_alt_ref, &opts)?;
        if has.get("track").truthy() || has.get("speed").truthy() {
            let type_ = if has.get("track").truthy() { "track" } else { "speed" };
            let mut t_fix = fix.clone();
            if fix.get(type_).is_undef() && !i_ext.is_undef() {
                let p = find_fix_v(it, &et, &s(type_), &times, &points, &i_ext, &i_dir, &geo_max_ext_secs)?;
                if p.truthy() {
                    t_fix = p;
                }
            }
            set_nv(it, &et, "GPSTrack", t_fix.get("track"), &opts)?;
            set_nv(it, &et, "GPSTrackRef", if !t_fix.get("track").is_undef() { s("T") } else { Value::Undef }, &opts)?;
            let (mut spd, mut rf) = (t_fix.get("speed"), Value::Undef);
            if !spd.is_undef() {
                // convert to specified units if necessary
                let speed_conv = lh(it, "speedConv");
                rf = et.path(&["OPTIONS", "GeoSpeedRef"]);
                if rf.truthy() && !hget(&speed_conv, &rf).is_undef() {
                    if hget(&speed_conv, &hget(&speed_conv, &rf)).truthy() {
                        rf = hget(&speed_conv, &rf);
                    }
                    spd = pl::mul(&spd, &hget(&speed_conv, &rf));
                } else {
                    rf = s("N"); // knots by default
                }
            }
            set_nv(it, &et, "GPSSpeed", spd, &opts)?;
            set_nv(it, &et, "GPSSpeedRef", rf, &opts)?;
        }
        if has.get("orient").truthy() {
            let mut t_fix = fix.clone();
            if fix.get("dir").is_undef() && !i_ext.is_undef() {
                let p = find_fix_v(it, &et, &s("dir"), &times, &points, &i_ext, &i_dir, &geo_max_ext_secs)?;
                if p.truthy() {
                    t_fix = p;
                }
            }
            set_nv(it, &et, "GPSImgDirection", t_fix.get("dir"), &opts)?;
            set_nv(it, &et, "GPSImgDirectionRef", if !t_fix.get("dir").is_undef() { s("T") } else { Value::Undef }, &opts)?;
            set_nv(it, &et, "CameraElevationAngle", t_fix.get("pitch"), &opts)?;
            // Note: GPSPitch and GPSRoll are non-standard, and must be user-defined
            set_nv(it, &et, "GPSPitch", t_fix.get("pitch"), &opts)?;
            set_nv(it, &et, "GPSRoll", t_fix.get("roll"), &opts)?;
        }
        if has.get("atemp").truthy() {
            let mut t_fix = fix.clone();
            if fix.get("atemp").is_undef() && !i_ext.is_undef() {
                // (not all fixes have atemp, so try interpolating specifically for this)
                let p = find_fix_v(it, &et, &s("atemp"), &times, &points, &i_ext, &i_dir, &geo_max_ext_secs)?;
                if p.truthy() {
                    t_fix = p;
                }
            }
            set_nv(it, &et, "AmbientTemperature", t_fix.get("atemp"), &opts)?;
        }
        if has.get("err").truthy() {
            set_nv(it, &et, "GPSHPositioningError", fix.get("err"), &opts)?;
        }
        if has.get("dop").truthy() {
            let (mut dop, mut mm) = (Value::Undef, Value::Undef);
            if !fix.get("pdop").is_undef() {
                dop = fix.get("pdop");
                mm = n(3);
            } else if !fix.get("hdop").is_undef() {
                if !fix.get("vdop").is_undef() {
                    let (h, v) = (fix.get("hdop"), fix.get("vdop"));
                    dop = Value::Num(pl::add(&pl::mul(&h, &h), &pl::mul(&v, &v)).to_f64().sqrt());
                    mm = n(3);
                } else {
                    dop = fix.get("hdop");
                    mm = n(2);
                }
            }
            if !dop.is_undef() {
                let mut a1 = vec![s("GPSMeasureMode"), mm];
                a1.extend(opts.flat());
                meth_void(it, &et, "SetNewValue", a1)?;
                let mut a2 = vec![s("GPSDOP"), dop.clone()];
                a2.extend(opts.flat());
                meth_void(it, &et, "SetNewValue", a2)?;
                // also set GPSHPositioningError if specified
                let mut hpos_err = et.path(&["OPTIONS", "GeoHPosErr"]);
                if hpos_err.truthy() {
                    re(r"gpsdop", "i").subst(&mut hpos_err, false, |_| b"GPSDOP".to_vec());
                    let gpsdop = dop.clone();
                    let handler = Value::Code(it.sub_ref("Image::ExifTool::SetWarning"));
                    let et2 = et.clone();
                    let opts2 = opts.clone();
                    pl::local_sig_warn(it, handler, |it| {
                        it.set_scalar("Image::ExifTool::evalWarning", Value::Undef);
                        //### eval GeoHPosErr ($GPSDOP)
                        let (r, _) = { let fs = runtime::file_scope(it, MODULE); pl::eval_site_in(it, &hpos_err, MODULE, Some(fs), &[("GPSDOP", gpsdop)], Want::Scalar) };
                        let hpe = r;
                        let mut e = pl::call(it, "Image::ExifTool::GetWarning", vec![])?;
                        if !e.truthy() {
                            e = it.get_scalar("main::@");
                        }
                        if e.truthy() {
                            let e = pl::call(it, "Image::ExifTool::CleanWarning", vec![e])?;
                            meth_void(it, &et2, "Warn", vec![cat(&[s("Error calculating GPSHPositioningError: "), e]), n(1)])?;
                        } else {
                            let mut a3 = vec![s("GPSHPositioningError"), hpe];
                            a3.extend(opts2.flat());
                            meth_void(it, &et2, "SetNewValue", a3)?;
                        }
                        Ok(())
                    })?;
                }
            }
        }
        if !xmp {
            let (mut lat_ref, mut lon_ref) = (Value::Undef, Value::Undef);
            if !fix.get("lat").is_undef() {
                lat_ref = s(if pl::gt(&fix.get("lat"), &n(0)) { "N" } else { "S" });
            }
            if !fix.get("lon").is_undef() {
                lon_ref = s(if pl::gt(&fix.get("lon"), &n(0)) { "E" } else { "W" });
            }
            set_nv(it, &et, "GPSLatitudeRef", lat_ref, &opts)?;
            set_nv(it, &et, "GPSLongitudeRef", lon_ref, &opts)?;
            set_nv(it, &et, "GPSDateStamp", gps_date.clone(), &opts)?;
            set_nv(it, &et, "GPSTimeStamp", gps_time.clone(), &opts)?;
            // set options to edit XMP:GPSDateTime only if it already exists
            opts.set("EditOnly", n(1));
            opts.set("Group", s("XMP"));
        }
        if !exif {
            set_nv(it, &et, "GPSDateTime", cat(&[gps_date, s(" "), gps_time]), &opts)?;
        }
        // set user-defined tags
        let mut uts: Vec<Value> = user_tag.keys().iter().map(|k| user_tag.get(k)).collect();
        crate::modules::write_exif::sort_str(&mut uts);
        for ut in uts {
            if !hget(&fix, &ut).is_undef() {
                pl::method_list(it, &et, "SetNewValue", vec![pl::substr(&ut, 1, None), hget(&fix, &ut)])?;
            }
        }
    } else {
        let mut opts = Opts::default();
        opts.set("IgnorePermanent", n(1));
        if !val.is_undef() {
            opts.set("Replace", n(2)); // remove existing new values
        }
        // reset user-defined GPX tags
        init_user_tags_v(it, &et)?; // (won't be set yet because we didn't read a GPX file)
        let user_tag = lh(it, "userTag");
        for k in user_tag.keys() {
            let ut = user_tag.get(&k);
            let mut a_ = vec![pl::substr(&ut, 1, None), Value::Undef];
            a_.extend(opts.flat());
            pl::method_list(it, &et, "SetNewValue", a_)?;
        }
        if write_group.truthy() {
            opts.set("Group", write_group.clone());
        }
        // reset any GPS values we might have already set
        for tag in [
            "GPSLatitude", "GPSLatitudeRef", "GPSLongitude", "GPSLongitudeRef", "GPSAltitude", "GPSAltitudeRef", "GPSDateStamp", "GPSTimeStamp", "GPSDateTime", "GPSTrack", "GPSTrackRef", "GPSSpeed", "GPSSpeedRef", "GPSImgDirection", "GPSImgDirectionRef", "GPSPitch", "GPSRoll", "CameraElevationAngle", "AmbientTemperature", "GPSHPositioningError", "GPSCoordinates", "GPSMeasureMode", "GPSDOP",
        ] {
            set_nv(it, &et, tag, Value::Undef, &opts)?;
        }
    }
    ret(err)
}

/// Convert Geotagging time synchronization value
/// Inputs: 0) exiftool object ref,
///         1) time difference string ("[+-]DD MM:HH:SS.ss"), geosync'd file name,
///            "GPSTIME@IMAGETIME", or "GPSTIME@FILENAME"
/// Returns: geosync hash:
///           Offset = Offset in seconds for latest synchronization (GPS - image time)
///           Points = hash of all sync offsets keyed by image times in seconds
///           Times = sorted list of image synchronization times (keys in Points hash)
/// Notes: calling this routine with more than one geosync'd file causes time drift
///        correction to be implemented
pub fn convert_geosync(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, val) = (arg(&a, 0), arg(&a, 1));
    let mut sync = pl::method(it, &et, "GetNewValue", vec![s("Geosync")])?;
    if !sync.truthy() {
        sync = Value::new_hash();
    }
    let (mut sync_file, mut gps_time, mut img_time) = (Value::Undef, Value::Undef, Value::Undef);
    let sec_per_day = runtime::lex_scalar(it, MODULE, "secPerDay").borrow().clone();

    if let Some(c) = m(r"(.*?)\@(.*)", "", &val) {
        gps_time = cap(&c, 1);
        if is_file(&cap(&c, 2)) {
            sync_file = cap(&c, 2);
        } else {
            img_time = cap(&c, 2);
        }
    // (take care because "-f '1:30'" crashes ActivePerl 5.10)
    } else if (m(r"^\d", "", &val).is_none() || m(r":", "", &val).is_none()) && is_file(&val) {
        sync_file = val.clone();
    }
    if gps_time.truthy() || !sync_file.is_undef() {
        // (this is a time synchronization vector)
        if !sync_file.is_undef() {
            // check the following tags in order to obtain the image timestamp
            let time_tags = ["SubSecDateTimeOriginal", "SubSecCreateDate", "SubSecModifyDate", "DateTimeOriginal", "CreateDate", "ModifyDate", "FileModifyDate"];
            let o = Value::new_hash();
            o.set("PrintConv", n(0));
            let mut args = vec![sync_file.clone(), o];
            args.extend(time_tags.iter().map(|t| s(t)));
            args.push(s("GPSDateTime"));
            args.push(s("GPSTimeStamp"));
            let info = pl::call(it, "Image::ExifTool::ImageInfo", args)?;
            if info.get("Error").truthy() {
                pl::warn(it, &format!("{}\n", info.get("Err").to_text()))?;
                return pl::ret_undef();
            }
            if !gps_time.truthy() {
                gps_time = if info.get("GPSDateTime").truthy() { info.get("GPSDateTime") } else { info.get("GPSTimeStamp") };
                if gps_time.truthy() && !info.get("GPSDateTime").truthy() {
                    gps_time = pl::concat(&gps_time, &s("Z"));
                }
            }
            if !gps_time.truthy() {
                pl::warn(it, &format!("No GPSTimeStamp in '{}\n", sync_file.to_text()))?;
                return pl::ret_undef();
            }
            for tag in time_tags {
                if info.get(tag).truthy() {
                    img_time = info.get(tag);
                    pl::method(it, &et, "VPrint", vec![n(2), s(&format!("Geosyncing with {tag} from '{}'\n", sync_file.to_text()))])?;
                    break;
                }
            }
            if !img_time.truthy() {
                pl::warn(it, &format!("No image timestamp in '{}'\n", sync_file.to_text()))?;
                return pl::ret_undef();
            }
        }
        // add date to date-less timestamps
        let (img_date_time, gps_date_time);
        let mut no_date = false;
        if let Some(c) = m(r"^(\d+:\d+:\d+)\s+\d+", "", &img_time) {
            img_date_time = img_time.clone();
            let date = cap(&c, 1);
            if m(r"^\d+:\d+:\d+\s+\d+", "", &gps_time).is_some() {
                gps_date_time = gps_time.clone();
            } else {
                gps_date_time = cat(&[date, s(" "), gps_time.clone()]);
            }
        } else if let Some(c) = m(r"^(\d+:\d+:\d+)\s+\d+", "", &gps_time) {
            img_date_time = cat(&[cap(&c, 1), s(" "), img_time.clone()]);
            gps_date_time = gps_time.clone();
        } else {
            // use a today's date (so hopefully the DST setting will be intuitive)
            let tm = localtime_now(it);
            let date = pl::sprintf("%.4d:%.2d:%.2d", &[pl::add(&tm[5], &n(1900)), pl::add(&tm[4], &n(1)), tm[3].clone()]);
            gps_date_time = cat(&[date.clone(), s(" "), gps_time.clone()]);
            img_date_time = cat(&[date, s(" "), img_time.clone()]);
            no_date = true;
        }
        // calculate Unix seconds since the epoch
        let mut img_secs = pl::call(it, "Image::ExifTool::GetUnixTime", vec![img_date_time.clone(), n(1)])?;
        if img_secs.is_undef() {
            pl::warn(it, &format!("Invalid image time '{}'\n", img_time.to_text()))?;
            return pl::ret_undef();
        }
        let mut gps_secs = pl::call(it, "Image::ExifTool::GetUnixTime", vec![gps_date_time.clone(), n(1)])?;
        if gps_secs.is_undef() {
            pl::warn(it, &format!("Invalid GPS time '{}'\n", gps_time.to_text()))?;
            return pl::ret_undef();
        }
        // add fractional seconds
        if let Some(c) = m(r"(\.\d+)", "", &gps_time) {
            gps_secs = pl::add(&gps_secs, &cap(&c, 1));
        }
        if let Some(c) = m(r"(\.\d+)", "", &img_time) {
            img_secs = pl::add(&img_secs, &cap(&c, 1));
        }
        // shift dates within 12 hours of each other if either timestamp was date-less
        if !seq(&gps_date_time, &gps_time) || !seq(&img_date_time, &img_time) {
            let mut diff = pl::modulo(&pl::sub(&img_secs, &gps_secs), &n(24 * 3600))?;
            if pl::gt(&diff, &n(12 * 3600)) {
                diff = pl::sub(&diff, &n(24 * 3600));
            }
            if pl::lt(&diff, &n(-12 * 3600)) {
                diff = pl::add(&diff, &n(24 * 3600));
            }
            if !seq(&gps_date_time, &gps_time) {
                gps_secs = pl::sub(&img_secs, &diff);
            } else {
                img_secs = pl::add(&gps_secs, &diff);
            }
        }
        // save the synchronization offset
        sync.set("Offset", pl::sub(&gps_secs, &img_secs));
        // save this synchronization point if either timestamp had a date
        if !no_date {
            let sp = sync.get_or_new_hash("Points");
            hset(&sp, &img_secs, sync.get("Offset"));
            // print verbose output
            if pl::gt(&opt(it, &et, "Verbose")?, &n(1)) {
                // print GPS and image timestamps in UTC
                let g = print_fix_time_v(it, &gps_secs)?;
                let i = print_fix_time_v(it, &img_secs)?;
                pl::method(it, &et, "VPrint", vec![n(1), s("Added Geosync point:\n"), s("  GPS time stamp:  "), g, s("\n"), s("  Image date/time: "), i, s("\n")])?;
            }
            // save sorted list of image sync times if we have more than one
            let mut times: Vec<Value> = sp.keys().into_iter().map(Value::from).collect();
            if times.len() > 1 {
                sort_num(&mut times);
                sync.set("Times", Value::new_array(times));
            }
        }
    } else {
        // (this is a simple time difference)
        let mut vals = re(r"(?=\d|\.\d)\d*(?:\.\d*)?", "").all(&val); // (allow decimal values too)
        if vals.is_empty() {
            pl::warn(it, "Invalid value (please refer to geotag documentation)\n")?;
            return pl::ret_undef();
        }
        let mut secs = n(0);
        for mult in [n(1), n(60), n(3600), sec_per_day.clone()] {
            secs = pl::add(&secs, &pl::mul(&mult, &vals.pop().unwrap_or_default()));
            if vals.is_empty() {
                break;
            }
        }
        // set constant sync offset
        sync.set("Offset", if m(r"^\s*-", "", &val).is_some() { pl::neg(&secs) } else { secs });
    }
    ret(sync)
}

/// Print fix time
/// Inputs: 0) time since the epoch
/// Returns: UTC time string with fractional seconds
pub fn print_fix_time(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    ret(print_fix_time_v(it, &arg(&a, 0))?)
}

fn print_fix_time_v(it: &mut Interp, time: &Value) -> ER<Value> {
    let t = pl::call(it, "Image::ExifTool::ConvertUnixTime", vec![time.clone(), Value::Undef, n(3)])?;
    Ok(pl::concat(&t, &s(" UTC")))
}

/// Print fix information
/// Inputs: 0) lookup for all fix points, 1-n) list of fix times
/// Returns: fix string (including leading indent and trailing newline)
pub fn print_fix(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let points = arg(&a, 0);
    ret(print_fix_v(it, &points, a.get(1..).unwrap_or(&[]))?)
}

fn print_fix_v(it: &mut Interp, points: &Value, times: &[Value]) -> ER<Value> {
    let mut st = s("");
    for time in times {
        let pft = print_fix_time_v(it, time)?;
        st = cat(&[st, s("    "), pft, s(" -")]);
        let fix = hget(points, time);
        if fix.truthy() {
            let mut keys: Vec<Value> = fix.keys().into_iter().map(Value::from).collect();
            crate::modules::write_exif::sort_str(&mut keys);
            for k in keys {
                let v = hget(&fix, &k);
                if !(pl::str_eq(&k, "time") || v.is_undef()) {
                    st = cat(&[st, s(" "), k, s("="), v]);
                }
            }
        }
        st = pl::concat(&st, &s("\n"));
    }
    Ok(st)
}

/// Initialize %userTag for reading user-defined GPX tags
/// Inputs: 0) ExifTool ref
pub fn init_user_tags(it: &mut Interp, a: Vec<Value>, want: Want) -> ER<Vec<Value>> {
    init_user_tags_v(it, &arg(&a, 0))?;
    pl::ret_nothing(want)
}

fn init_user_tags_v(it: &mut Interp, et: &Value) -> ER<()> {
    let user_tag = lh(it, "userTag");
    for k in user_tag.keys() {
        user_tag.delete(&k);
    }
    let gut = et.path(&["OPTIONS", "GeoUserTag"]);
    if gut.truthy() {
        let xml_tag = lh(it, "xmlTag");
        for item in pl::split(&re(r"\s*,\s*", ""), &gut, 0) {
            let Some(c) = m(r"^(.+)=(.+)$", "", &item) else { continue };
            if hget(&xml_tag, &pl::lc(&cap(&c, 2))).truthy() {
                pl::method(it, et, "Warn", vec![cat(&[s("User-defined GPX tag '"), cap(&c, 2), s("' conflicts with existing tag")])])?;
                continue;
            }
            hset(&user_tag, &pl::lc(&cap(&c, 2)), cat(&[s("_"), cap(&c, 1)])); // (leading underline prevents conflicts)
        }
    }
    Ok(())
}

pub fn register(it: &mut Interp) {
    it.define_native("Image::ExifTool::Geotag::JITTER", jitter);
    it.define_native("Image::ExifTool::Geotag::SplitCSV", split_csv);
    it.define_native("Image::ExifTool::Geotag::LoadTrackLog", load_track_log);
    it.define_native("Image::ExifTool::Geotag::GetTime", get_time);
    it.define_native("Image::ExifTool::Geotag::ApplySyncCorr", apply_sync_corr);
    it.define_native("Image::ExifTool::Geotag::ScanOutwards", scan_outwards);
    it.define_native("Image::ExifTool::Geotag::FindFix", find_fix);
    it.define_native("Image::ExifTool::Geotag::SetGeoValues", set_geo_values);
    it.define_native("Image::ExifTool::Geotag::ConvertGeosync", convert_geosync);
    it.define_native("Image::ExifTool::Geotag::PrintFixTime", print_fix_time);
    it.define_native("Image::ExifTool::Geotag::PrintFix", print_fix);
    it.define_native("Image::ExifTool::Geotag::InitUserTags", init_user_tags);
}
