//! Image::ExifTool::LigoGPS (Image/ExifTool/LigoGPS.pm), ported sub by sub (see docs/port/PORTING.md).
//!
//! Read LIGOGPSINFO timed GPS records (encrypted, enciphered, fuzzed or
//! JSON). QuickTime.pm's native `Image::ExifTool::LigoGPS::AUTOLOAD`
//! still loads this module on demand; nothing here replaces it.

use crate::interp::eval::ER;
use crate::interp::{Interp, Want};
use crate::modules::quick_time_stream::{cat, convert_lat_lon, len_of, m, mt, n, new_doc, opt, s, warn};
use crate::perl::Value;
use crate::pl::{self, arg, cap, re, ret};
use crate::runtime;

const KNOTS_TO_KPH: f64 = 1.852; // knots --> km/h

/// `pos($$dataPt)` as the interpreter keeps it.
pub(crate) fn get_pos(it: &mut Interp, data_pt: &Value) -> Option<usize> {
    let (v, _) = pl::eval_site(it, &s("pos($$d)"), "main", &[("d", data_pt.clone())], Want::Scalar);
    if v.is_undef() {
        None
    } else {
        Some(v.to_i64().max(0) as usize)
    }
}

/// `pos($$dataPt) = POS` (or undef).
pub(crate) fn set_pos(it: &mut Interp, data_pt: &Value, pos: Option<usize>) {
    let p = match pos {
        Some(p) => n(p as i64),
        None => Value::Undef,
    };
    let saved = it.get_scalar("main::@");
    pl::eval_site(it, &s("pos($$d) = $p"), "main", &[("d", data_pt.clone()), ("p", p)], Want::Scalar);
    it.set_scalar("main::@", saved);
}

/// Clean up cipher variables and print warning if deciphering was unsuccessful
/// Inputs: 0) ExifTool ref
pub fn cleanup_cipher(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let et = arg(&a, 0);
    let ci = et.get("LigoCipher");
    if ci.truthy() && ci.get("next").truthy() {
        warn(it, &et, s("Not enough GPS points to determine cipher for decoding LIGOGPSINFO"))?;
    }
    ret(et.delete("LigoCipher"))
}

/// Un-do LIGOGPS fuzzing
/// Inputs: 0) fuzzed latitude, 1) fuzzed longitude, 2) scale factor
/// Returns: 0) latitude, 1) longitude
pub fn unfuzz_ligo_gps(_it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (lat, lon, scl) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    let (la, lo) = unfuzz(&lat, &lon, &scl)?;
    Ok(vec![la, lo])
}

fn unfuzz(lat: &Value, lon: &Value, scl: &Value) -> ER<(Value, Value)> {
    let lat2 = pl::mul(&pl::int(&pl::div(lat, &n(10))?), &n(10));
    let lon2 = pl::mul(&pl::int(&pl::div(lon, &n(10))?), &n(10));
    Ok((pl::add(&lat2, &pl::mul(&pl::sub(lon, &lon2), scl)), pl::add(&lon2, &pl::mul(&pl::sub(lat, &lat2), scl))))
}

/// Decrypt LIGOGPSINFO record (starting with "####")
/// Inputs: 0) encrypted GPS record incuding 8-byte header
/// Returns: decrypted record including 4-byte uint32 header, or undef on error
pub fn decrypt_ligo_gps(_it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    ret(decrypt(&arg(&a, 0))?)
}

fn decrypt(st: &Value) -> ER<Value> {
    let mut num = pl::unpack1("x4V", st)?.to_i64();
    if num < 4 {
        return Ok(Value::Undef);
    }
    if num > 0x84 {
        num = 0x84; // (be safe)
    }
    let inp: Vec<i64> = pl::unpack(&format!("x8C{num}"), st)?.iter().map(|v| v.to_i64()).collect();
    let mut inp = std::collections::VecDeque::from(inp);
    let mut out: Vec<i64> = Vec::new();
    let sh = |q: &mut std::collections::VecDeque<i64>| q.pop_front().unwrap_or(0);
    while !inp.is_empty() {
        let b = sh(&mut inp); // get next byte in data
        // upper 3 bits steer the decryption for this round
        let steering_bits = b & 0xe0;
        if steering_bits >= 0xc0 {
            if inp.len() < 4 {
                return Ok(Value::Undef); // next 4 bytes are encrypted data
            }
            let a0 = (sh(&mut inp) | b & 0x01) ^ 0x20;
            let a1 = (sh(&mut inp) | b & 0x02) ^ 0x20;
            let a2 = (sh(&mut inp) | b & 0x0c) ^ 0x20;
            let a3 = (sh(&mut inp) ^ 0x20) | b & 0x30;
            out.extend([a0, a1, a2, a3]);
        } else if steering_bits >= 0x40 {
            if inp.len() < 3 {
                return Ok(Value::Undef); // next 3 bytes are encrypted data
            }
            if steering_bits == 0x40 {
                let a1 = (sh(&mut inp) | b & 0x01) ^ 0x20;
                let a2 = (sh(&mut inp) | b & 0x06) ^ 0x20;
                let a3 = (sh(&mut inp) | b & 0x18) ^ 0x20;
                out.extend([0x20, a1, a2, a3]);
            } else if steering_bits == 0x60 {
                let a0 = (sh(&mut inp) | b & 0x03) ^ 0x20;
                let a2 = (sh(&mut inp) | b & 0x04) ^ 0x20;
                let a3 = (sh(&mut inp) | b & 0x18) ^ 0x20;
                out.extend([a0, 0x20, a2, a3]);
            } else if steering_bits == 0x80 {
                let a0 = (sh(&mut inp) | b & 0x03) ^ 0x20;
                let a1 = (sh(&mut inp) | b & 0x0c) ^ 0x20;
                let a3 = (sh(&mut inp) | b & 0x10) ^ 0x20;
                out.extend([a0, a1, 0x20, a3]);
            } else {
                let a0 = (sh(&mut inp) | b & 0x01) ^ 0x20;
                let a1 = (sh(&mut inp) | b & 0x06) ^ 0x20;
                let a2 = (sh(&mut inp) | b & 0x18) ^ 0x20;
                out.extend([a0, a1, a2, 0x20]);
            }
        } else if steering_bits == 0x00 {
            if inp.is_empty() {
                return Ok(Value::Undef); // next byte is encrypted data
            }
            out.push(sh(&mut inp) | b & 0x13);
        } else {
            return Ok(Value::Undef); // (shouldn't happen)
        }
    }
    let vals: Vec<Value> = out.into_iter().map(n).collect();
    pl::pack("C*", &vals)
}

/// `$#$order = N` (truncate or extend an array reference).
fn set_last_index(a: &Value, last: i64) {
    if let Value::ArrayRef(r) = a {
        r.borrow_mut().resize((last + 1).max(0) as usize, Value::Undef);
    }
}

/// Determine correct ordering of enciphered digits (unit digits of seconds)
/// Inputs: 0) starting character code, 1) lookup for next character(s) in sequence
///         2) i/o list of ordered characters, 3) hash of used characters
/// Returns: true if a consistent ordering was found
/// - loops through all possible orders based on $next sequence until a complete
///   cycle is established
/// - this complexity is necessary because GPS may skip some seconds
pub fn order_cipher_digits(_it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    ret(n(order_digits(arg(&a, 0), &arg(&a, 1), &arg(&a, 2), arg(&a, 3))))
}

fn order_digits(mut ch: Value, next: &Value, order: &Value, mut did: Value) -> i64 {
    if !did.truthy() {
        did = Value::new_hash();
    }
    while next.get(ch.to_bytes()).truthy() {
        if order.len() < 10 {
            if did.get(ch.to_bytes()).truthy() {
                break;
            }
        } else {
            // success if we have cycled through all 10 digits and back to the first
            if order.len() == 10 && ch.to_bytes() == order.at(0).to_bytes() {
                return 1;
            }
            break;
        }
        order.push(ch.clone());
        did.set(ch.to_bytes(), n(1));
        // continue with next character if there is only one possibility
        let nx = next.get(ch.to_bytes());
        if nx.len() == 1 {
            ch = nx.at(0);
            continue;
        }
        // otherwise, test all possibilities
        let nn = order.len() as i64 - 1;
        for c in nx.items() {
            let did2 = did.hash_copy(); // make a copy of the used-character lookup
            if order_digits(c, next, order, did2) != 0 {
                return 1;
            }
            set_last_index(order, nn); // restore order and try next possibility
        }
        break;
    }
    0 // failure
}

/// Decipher and parse LIGOGPSINFO record (starting with "####")
/// Inputs: 0) ExifTool ref, 1) enciphered string, 2) tag table ref
///         3) true if GPS coordinates don't need de-fuzzing
/// Returns: true if this looked like an enciphered string
/// Notes: handles contained tags, but may defer handling until full cipher is known
pub fn decipher_ligo_gps(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, mut st, tag_tbl, no_fuzz) = (arg(&a, 0), arg(&a, 1), arg(&a, 2), arg(&a, 3));

    // (enciphered characters must be in the range 0x30-0x5f ('0' - '_'))
    let Some(c) = mt(r"^####.{4}([0-_])[0-_]{3}/[0-_]{2}/[0-_]{2} ..([0-_])..([0-_]).([0-_]) ", "s", &st) else {
        return pl::ret_undef();
    };
    if cap(&c, 2).to_bytes() != cap(&c, 3).to_bytes() {
        return pl::ret_undef(); // (colons in time string must be the same)
    }

    let mut cipher_info = et.get("LigoCipher");
    if !cipher_info.truthy() {
        cipher_info = Value::new_hash();
        cipher_info.set("cache", Value::new_array(vec![]));
        cipher_info.set("next", Value::new_hash());
        et.set("LigoCipher", cipher_info.clone());
        let code = Value::Code(it.sub_ref("Image::ExifTool::LigoGPS::CleanupCipher"));
        m(it, &et, "AddCleanup", vec![code])?;
    }
    let mut decipher = cipher_info.get("decipher");
    let cache = cipher_info.get("cache");

    // determine the cipher code table based on the advancing 1's digit of seconds
    if !decipher.truthy() {
        cache.push(st.clone()); // cache records until we can decipher them
        let next = cipher_info.get("next");
        let (millennium, colon, ch2) = (cap(&c, 1), cap(&c, 2), cap(&c, 4));
        // determine the cipher lookup table
        // (only characters in range 0x30-0x5f are encrypted)
        let ch1 = cipher_info.get("ch1");
        cipher_info.set("ch1", ch2.clone());
        if ch1.is_undef() || ch1.to_bytes() == ch2.to_bytes() {
            return ret(n(1)); // ignore duplicate sequential digits
        }
        let nx = next.get(ch1.to_bytes());
        if nx.truthy() {
            let c2 = ch2.to_bytes();
            if nx.items().iter().any(|x| x.to_bytes().windows(c2.len().max(1)).any(|w| w == &c2[..])) {
                return ret(n(1)); // don't add twice
            }
            nx.push(ch2.clone());
        } else {
            next.set(ch1.to_bytes(), Value::new_array(vec![ch2.clone()]));
        }
        // must wait until the lookup contains all 10 digits
        if next.len() < 10 {
            return ret(n(1));
        }
        // protect against trying to decipher bad data
        if next.len() > 10 {
            cipher_info.set("next", Value::new_hash());
            return ret(n(1));
        }
        let order = Value::new_array(vec![]);
        let mut two = Value::Undef;
        if order_digits(ch1.clone(), &next, &order, Value::Undef) == 0 {
            return ret(n(1));
        }
        // get index of enciphered "2" in ordered array
        for i in 0..10 {
            if order.at(i).to_bytes() == millennium.to_bytes() {
                two = n(i);
                break;
            }
        }
        if two.is_undef() {
            warn(it, &et, s("Problem deciphering LIGOGPSINFO"))?;
            return ret(n(1));
        }
        cipher_info.delete("next"); // all done with 'next' lookup
        let dec = Value::new_hash();
        dec.set(colon.to_bytes(), s(":")); // (':' is the time separator)
        for i in 0..10i64 {
            let ch = order.at((i + two.to_i64() - 2 + 10) % 10);
            dec.set(ch.to_bytes(), Value::bytes([(i + 0x30) as u8]));
        }
        // may also know the lat/lon quadrant from the signs of the coordinates
        let cb = colon.to_bytes();
        let mut pat = b" ([0-_])".to_vec();
        pat.extend_from_slice(&cb);
        pat.extend_from_slice(b"(-?).*? ([0-_])");
        pat.extend_from_slice(&cb);
        pat.extend_from_slice(b"(-?)");
        if let Some(q) = pl::re_dyn(&pat, "")?.m(&st) {
            let (c1, c2, c3, c4) = (cap(&q, 1), cap(&q, 2), cap(&q, 3), cap(&q, 4));
            dec.set(c1.to_bytes(), s(if c2.truthy() { "S" } else { "N" }));
            dec.set(c3.to_bytes(), s(if c4.truthy() { "W" } else { "E" }));
            if !(c2.truthy() || c4.truthy()) {
                let (ns, ew) = (c1, c3);
                let gq = et.get("OPTIONS").get("GPSQuadrant");
                let qm = if gq.truthy() { mt(r"^([NS])([EW])$", "i", &gq) } else { None };
                if let Some(qm) = qm {
                    dec.set(ns.to_bytes(), pl::uc(&cap(&qm, 1)));
                    dec.set(ew.to_bytes(), pl::uc(&cap(&qm, 2)));
                } else {
                    warn(it, &et, s("May need to set API GPSQuadrant option (eg. \"NW\")"))?;
                }
            }
        }
        // fill in unknown entries with '?' (only chars 0x30-0x5f are enciphered)
        for ch in 0x30u8..=0x5f {
            if dec.get([ch]).is_undef() {
                dec.set([ch], s("?"));
            }
        }
        decipher = dec.clone();
        cipher_info.set("decipher", dec);
        st = shift(&cache); // start deciphering at oldest cache entry
    }

    // apply reverse cipher and extract GPS information
    loop {
        let pre = pl::substr(&st, 4, Some(4)); // save second 4 bytes of header
        st = pl::substr(&st, 8, None);
        re(r"\0+$", "").subst(&mut st, false, |_| Vec::new()); // remove 8-byte header and null padding
        let d = decipher.clone();
        re(r"([0-_])", "").subst(&mut st, true, |c| d.get(cap(c, 1).to_bytes()).to_bytes().to_vec()); // decipher
        if pl::gt(&et.get("OPTIONS").get("Verbose"), &n(1)) {
            let h = pl::unpack1("H8", &pre)?;
            m(it, &et, "VPrint", vec![n(1), cat(&[&et.get("INDENT"), &s("(Deciphered: "), &h, &s(" "), &st, &s(")\n")])])?;
        }
        // add back leading 4 bytes (int16u counter plus 2 unknown bytes), and parse
        parse_ligo_gps(it, vec![et.clone(), cat(&[&pre, &st]), tag_tbl.clone(), no_fuzz.clone()], Want::Void)?;
        st = shift(&cache);
        if !st.truthy() {
            break;
        }
    }

    ret(n(1))
}

/// `shift @$a`
fn shift(a: &Value) -> Value {
    match a {
        Value::ArrayRef(r) => {
            let mut v = r.borrow_mut();
            if v.is_empty() {
                Value::Undef
            } else {
                v.remove(0)
            }
        }
        _ => Value::Undef,
    }
}

/// Parse decrypted/deciphered (but not defuzzed) LIGOGPSINFO record
/// (record starts with 4-byte int32u counter followed by date/time, etc)
/// Inputs: 0) ExifTool ref, 1) GPS string, 2) tag table ref,
///         3) flags: 0x01=not fuzzed, 0x02=spd in km/h
/// Returns: nothing
pub fn parse_ligo_gps(it: &mut Interp, a: Vec<Value>, want: Want) -> ER<Vec<Value>> {
    let (et, st, tag_tbl, mut flags) = (arg(&a, 0), arg(&a, 1), arg(&a, 2), arg(&a, 3));

    // example string input
    // "....2022/09/19 12:45:24 N:31.285065 W:124.759483 46.93 km/h x:-0.000 y:-0.000 z:-0.000"
    let Some(c) = mt(r"^.{4}(\S+ \S+)\s+([NS?]):(-?)([.\d]+)\s+([EW?]):(-?)([\.\d]+)\s+([.\d]+)", "s", &st) else {
        warn(it, &et, s("LIGOGPSINFO format error"))?;
        return pl::ret_nothing(want);
    };
    if !flags.truthy() {
        flags = n(0);
    }
    let (mut time, lat_ref, lat_neg, mut lat, lon_ref, lon_neg, mut lon, spd) = (cap(&c, 1), cap(&c, 2), cap(&c, 3), cap(&c, 4), cap(&c, 5), cap(&c, 6), cap(&c, 7), cap(&c, 8));
    let gps_scl = |k: &Value| -> Option<f64> {
        match &*k.to_bytes() {
            b"1" => Some(1.524855137),
            b"2" => Some(1.456027985),
            b"3" => Some(1.15368),
            _ => None,
        }
    };
    let fl = flags.to_i64();
    let mut spd_scl = if fl & 0x01 != 0 {
        if fl & 0x02 != 0 {
            n(1)
        } else {
            Value::Num(KNOTS_TO_KPH)
        }
    } else {
        Value::Num(1.85407333)
    };
    new_doc(&et);
    pl::tr(&mut time, "/", ":", "");
    // convert from DDMM.MMMMMM to DD.DDDDDD if necessary
    // (speed wasn't scaled in my 1 sample with this format)
    if re(r"^\d{3}", "").is_match(&lat) {
        convert_lat_lon(&mut lat, &mut lon)?;
        spd_scl = n(1);
    }
    if fl & 0x01 == 0 {
        // unfuzz the coordinates if necessary
        let o = et.get("OPTIONS").get("LigoGPSScale");
        let e = et.get("LigoGPSScale");
        let mut scl = if o.truthy() { o } else if e.truthy() { e } else { n(1) };
        if let Some(x) = gps_scl(&scl) {
            scl = Value::Num(x);
        }
        let (a1, a2) = unfuzz(&lat, &lon, &scl)?;
        lat = a1;
        lon = a2;
    }
    // a final sanity check
    if pl::gt(&lat, &n(90)) || pl::gt(&lon, &n(180)) {
        warn(it, &et, s("LIGOGPSINFO coordinates out of range"))?;
        return pl::ret_nothing(want);
    }
    et.set("SET_GROUP1", s("LIGO"));
    let h = |it: &mut Interp, tag: &str, v: Value| m(it, &et, "HandleTag", vec![tag_tbl.clone(), s(tag), v]);
    h(it, "GPSDateTime", time)?;
    // (ignore N/S/E/W if coordinate is signed)
    let ls = if lat_neg.truthy() || lat_ref.eq_str("S") { -1 } else { 1 };
    h(it, "GPSLatitude", pl::mul(&lat, &n(ls)))?;
    let ws = if lon_neg.truthy() || lon_ref.eq_str("W") { -1 } else { 1 };
    h(it, "GPSLongitude", pl::mul(&lon, &n(ws)))?;
    h(it, "GPSSpeed", pl::mul(&spd, &spd_scl))?;
    if let Some(c) = mt(r"\bA:(\S+)", "", &st) {
        h(it, "GPSTrack", cap(&c, 1))?;
    }
    if let Some(c) = mt(r"\bH:(\S+)", "", &st) {
        h(it, "GPSAltitude", cap(&c, 1))?;
    }
    if let Some(c) = mt(r"\bM:(\S+)", "", &st) {
        h(it, "MagneticVariation", cap(&c, 1))?;
    }
    // (have a sample where tab is used to separate acc components)
    if let Some(c) = mt(r"x:(\S+)\sy:(\S+)\sz:(\S+)", "", &st) {
        h(it, "Accelerometer", cat(&[&cap(&c, 1), &s(" "), &cap(&c, 2), &s(" "), &cap(&c, 3)]))?;
    }
    ret(et.delete("SET_GROUP1"))
}

/// Process GKU dashcam trailer containing JSON-format LigoGPS
/// Inputs: 0) ExifTool ref, 1) dirInfo ref, 2) tag table ref
/// Returns: 1 on success
pub fn process_gku(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info, tag_tbl) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    let data_pt = dir_info.get("DataPt");
    let data = data_pt.deref();
    let pos = pl::unpack1("V", &data)?;
    if pl::gt(&pl::add(&pos, &n(13)), &n(len_of(&data))) || !pl::substr(&data, pos.to_i64(), Some(13)).eq_str("LIGOGPSINFO {") {
        return ret(n(0));
    }
    set_pos(it, &data_pt, Some(pos.to_i64().max(0) as usize));
    process_ligo_json(it, vec![et, dir_info, tag_tbl], Want::Scalar)
}

/// Process LIGOGPSINFO data (non-JSON format)
/// Inputs: 0) ExifTool object ref, 1) dirInfo ref, 2) tag table ref
///         3) 1=LIGOGPS lat/lon/spd weren't fuzzed
/// Returns: 1 on success
/// Notes:  The directory data should start with the string "LIGOGPSINFO\0"
pub fn process_ligo_gps(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info, tag_tbl, mut no_fuzz) = (arg(&a, 0), arg(&a, 1), arg(&a, 2), arg(&a, 3));
    let data_pt = dir_info.get("DataPt");
    let data = data_pt.deref();
    let ds = dir_info.get("DirStart");
    let mut pos = pl::add(&if ds.truthy() { ds } else { n(0) }, &n(0x14));
    if pl::gt(&pos, &n(len_of(&data))) {
        return pl::ret_undef();
    }
    let cipher_info = et.get("LigoCipher");
    let dn = dir_info.get("DirName");
    let dir_name = if dn.truthy() { dn } else { s("LigoGPS") };
    if !dir_info.get("DirID").truthy() {
        et.get_or_new_array("PATH").push(dir_name.clone());
    }
    // not fuzzed if header =~ /LIGOGPSINFO\0\0\0\0[\x01\x14]/ (\x01=BlueSkySeaDV688)
    if re(r"^\0\0\0[\x01\x14]", "").is_match(&pl::substr(&data, pos.to_i64() - 8, Some(4))) {
        no_fuzz = n(1);
    }
    m(it, &et, "VerboseDir", vec![dir_name.clone()])?;
    while pl::le(&pl::add(&pos, &n(0x84)), &n(len_of(&data))) {
        let mut dat = pl::substr(&data, pos.to_i64(), Some(0x84));
        'rec: {
            if !re(r"^####", "").is_match(&dat) {
                if !re(r"^.{4}\d{4}/\d{2}/\d{2} ", "s").is_match(&dat) {
                    break 'rec; // (have seen blank records filled with zeros, so keep trying)
                }
                // non-encrypted format written by Redtiger F9 4K
                re(r"\0+$", "").subst(&mut dat, false, |_| Vec::new()); // remove trailing nulls
                parse_ligo_gps(it, vec![et.clone(), dat, tag_tbl.clone(), n(0x03)], Want::Void)?;
                break 'rec;
            }
            // decipher if we already know the encryption
            if cipher_info.truthy() && cipher_info.get("decipher").truthy() && scalar1(decipher_ligo_gps(it, vec![et.clone(), dat.clone(), tag_tbl.clone(), no_fuzz.clone()], Want::Scalar))?.truthy() {
                break 'rec;
            }
            let st = decrypt(&dat)?;
            if st.is_undef() {
                decipher_ligo_gps(it, vec![et.clone(), dat.clone(), tag_tbl.clone(), no_fuzz.clone()], Want::Scalar)?; // try to decipher
                break 'rec;
            }
            if pl::gt(&et.get("OPTIONS").get("Verbose"), &n(1)) {
                let v = pl::unpack1("V", &st)?;
                m(it, &et, "VPrint", vec![n(1), cat(&[&et.get("INDENT"), &s("(Decrypted: ")]), v, s(" "), pl::substr(&st, 4, None), s(")\n")])?;
            }
            parse_ligo_gps(it, vec![et.clone(), st, tag_tbl.clone(), no_fuzz.clone()], Want::Void)?;
        }
        pos = pl::add(&pos, &n(0x84));
    }
    if !dir_info.get("DirID").truthy() {
        if let Value::ArrayRef(r) = et.get("PATH") {
            r.borrow_mut().pop();
        }
    }
    et.delete("DOC_NUM");
    ret(n(1))
}

fn scalar1(r: ER<Vec<Value>>) -> ER<Value> {
    Ok(r?.pop().unwrap_or_default())
}

/// Process LIGOGPSINFO JSON-format GPS (Yada RoadCam Pro 4K BT58189)
/// Inputs: 0) ExifTool ref, 1) dirInfo ref, 2) tag table ref
/// Returns: 1 on success
pub fn process_ligo_json(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info, tag_tbl) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    let data_pt = dir_info.get("DataPt");
    let data = data_pt.deref();
    let _dir_len = dir_info.get("DirLen");
    runtime::require(it, "Image::ExifTool::Import")?;
    let start = get_pos(it, &data_pt);
    m(it, &et, "VerboseDir", vec![s("LIGO_JSON"), Value::Undef, n(len_of(&data) - start.unwrap_or(0) as i64)])?;
    et.set("SET_GROUP1", s("LIGO"));
    let mut p = start.unwrap_or(0);
    let mut end_pos: Option<usize> = None;
    while let Some((c, e)) = crate::modules::quick_time_stream::match_from(r"LIGOGPSINFO (\{.*?\})", "", &data, p) {
        p = e;
        let json = cap(&c, 1);
        let dbase = Value::new_hash();
        pl::call(it, "Image::ExifTool::Import::ReadJSON", vec![pl::sref(json), dbase.clone()])?;
        let info = dbase.get("*");
        if !info.truthy() {
            continue;
        }
        // my sample contains the following JSON fields (in this order):
        // Hour Minute Second Year Month Day (GPS UTC time)
        // status NS EW Latitude Longitude Speed (speed in knots)
        // GsensorX GsensorY GsensorZ (units? - only seen "000" for all)
        // MHour MMinute MSecond MYear MMonth MDay (local dashcam clock time)
        // OLatitude OLongitude (? same values as Latitude/Longitude)
        if !(!info.get("status").is_undef() && info.get("status").eq_str("A")) {
            continue; // only read if GPS is active
        }
        new_doc(&et);
        let h = |it: &mut Interp, tag: &str, v: Value| m(it, &et, "HandleTag", vec![tag_tbl.clone(), s(tag), v]);
        let keys = ["Year", "Month", "Day", "Hour", "Minute", "Second"];
        if keys.iter().filter(|k| !info.get(k).is_undef()).count() == 6 {
            // this is the GPS time in UTC
            let v: Vec<Value> = keys.iter().map(|k| info.get(k)).collect();
            h(it, "GPSDateTime", pl::sprintf("%.4d:%.2d:%.2d %.2d:%.2d:%.2dZ", &v))?;
        }
        let is_s = info.get("NS").truthy() && info.get("NS").eq_str("S");
        let is_w = info.get("EW").truthy() && info.get("EW").eq_str("W");
        if info.get("Latitude").truthy() && info.get("Longitude").truthy() {
            let mut lat = info.get("Latitude");
            if is_s {
                lat = pl::neg(&lat);
            }
            let mut lon = info.get("Longitude");
            if is_w {
                lon = pl::neg(&lon);
            }
            h(it, "GPSLatitude", lat)?;
            h(it, "GPSLongitude", lon)?;
        }
        if !info.get("Speed").is_undef() {
            h(it, "GPSSpeed", pl::mul(&info.get("Speed"), &Value::Num(KNOTS_TO_KPH)))?;
        }
        if !info.get("GsensorX").is_undef() && !info.get("GsensorY").is_undef() && !info.get("GsensorZ").is_undef() {
            // (don't know conversion factor for accel data, so leave it raw for now)
            h(it, "Accelerometer", cat(&[&info.get("GsensorX"), &s(" "), &info.get("GsensorY"), &s(" "), &info.get("GsensorZ")]))?;
        }
        let keys = ["MYear", "MMonth", "MDay", "MHour", "MMinute", "MSecond"];
        if keys.iter().filter(|k| !info.get(k).is_undef()).count() == 6 {
            // this is the dashcam clock time (local time zone)
            let v: Vec<Value> = keys.iter().map(|k| info.get(k)).collect();
            h(it, "DateTimeOriginal", pl::sprintf("%.4d:%.2d:%.2d %.2d:%.2d:%.2d", &v))?;
        }
        if !info.get("OLatitude").is_undef() && !info.get("OLongitude").is_undef() {
            let mut lat = info.get("OLatitude");
            if is_s {
                lat = pl::neg(&lat);
            }
            let mut lon = info.get("OLongitude");
            if is_w {
                lon = pl::neg(&lon);
            }
            h(it, "GPSLatitude2", lat)?;
            h(it, "GPSLongitude2", lon)?;
        }
        if !opt(it, &et, "ExtractEmbedded")?.truthy() {
            m(it, &et, "Warn", vec![s("Use the ExtractEmbedded option to extract all timed GPS"), n(3)])?;
            end_pos = Some(p);
            break;
        }
    }
    // (pos is reset by the failed match, or left after the last one)
    set_pos(it, &data_pt, end_pos);
    et.delete("DOC_NUM");
    et.delete("SET_GROUP1");
    ret(n(1))
}

pub fn register(it: &mut Interp) {
    it.define_native("Image::ExifTool::LigoGPS::CleanupCipher", cleanup_cipher);
    it.define_native("Image::ExifTool::LigoGPS::UnfuzzLigoGPS", unfuzz_ligo_gps);
    it.define_native("Image::ExifTool::LigoGPS::DecryptLigoGPS", decrypt_ligo_gps);
    it.define_native("Image::ExifTool::LigoGPS::OrderCipherDigits", order_cipher_digits);
    it.define_native("Image::ExifTool::LigoGPS::DecipherLigoGPS", decipher_ligo_gps);
    it.define_native("Image::ExifTool::LigoGPS::ParseLigoGPS", parse_ligo_gps);
    it.define_native("Image::ExifTool::LigoGPS::ProcessGKU", process_gku);
    it.define_native("Image::ExifTool::LigoGPS::ProcessLigoGPS", process_ligo_gps);
    it.define_native("Image::ExifTool::LigoGPS::ProcessLigoJSON", process_ligo_json);
}
