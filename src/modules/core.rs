//! The core Perl modules ExifTool uses, as natives: File::Basename, Cwd,
//! Time::Local, POSIX (the functions ExifTool calls), Scalar::Util...
//!
//! These are perl's own library, not ExifTool's; each follows its perl
//! implementation on Unix.

use crate::interp::eval::{die, ER};
use crate::interp::{Interp, Want};
use crate::perl::Value;

fn arg(a: &[Value], i: usize) -> Value {
    a.get(i).cloned().unwrap_or_default()
}

fn ret(v: impl Into<Value>) -> ER<Vec<Value>> {
    Ok(vec![v.into()])
}

/// File::Basename's `_strip_trailing_sep`: `s{(.)/*\z}{$1}s`.
fn strip_trailing_sep(p: &mut Vec<u8>) {
    while p.len() > 1 && p.last() == Some(&b'/') {
        p.pop();
    }
}

/// `fileparse($path, @suffixes)` → (basename, dirpath, suffix).
fn fileparse(path: &[u8], suffixes: &[Vec<u8>], regex_suffixes: bool) -> (Vec<u8>, Vec<u8>, Vec<u8>) {
    let (dir, base) = match path.iter().rposition(|&b| b == b'/') {
        Some(p) => (path[..=p].to_vec(), path[p + 1..].to_vec()),
        None => (b"./".to_vec(), path.to_vec()),
    };
    let mut base = base;
    let mut tail = Vec::new();
    for s in suffixes {
        // Each suffix is a pattern anchored at the end: ($sfx)$
        let pat = if regex_suffixes { s.clone() } else { crate::perl::regex::quotemeta(s) };
        let mut p = b"(".to_vec();
        p.extend_from_slice(&pat);
        p.extend_from_slice(b")$");
        if let Ok(re) = crate::perl::regex::compile(&p, "s") {
            if let Ok(Some(m)) = re.match_at(&base, 0) {
                if let Some((a, _)) = m[0] {
                    tail = base[a..].iter().chain(tail.iter()).copied().collect();
                    base.truncate(a);
                }
            }
        }
    }
    (base, dir, tail)
}

fn dirname(path: &[u8]) -> Vec<u8> {
    let (base, mut dir, _) = fileparse(path, &[], false);
    strip_trailing_sep(&mut dir);
    if base.is_empty() {
        let (_, mut d2, _) = fileparse(&dir, &[], false);
        strip_trailing_sep(&mut d2);
        return d2;
    }
    dir
}

fn basename(path: &[u8], suffixes: &[Vec<u8>]) -> Vec<u8> {
    let mut p = path.to_vec();
    strip_trailing_sep(&mut p);
    let (mut base, dir, sfx) = fileparse(&p, suffixes, false);
    if !sfx.is_empty() && base.is_empty() {
        base = sfx;
    }
    if base.is_empty() {
        base = dir;
    }
    base
}

fn md5(data: &[u8]) -> Vec<u8> {
    use md5::{Digest, Md5};
    Md5::digest(data).to_vec()
}

fn concat(a: &[Value]) -> Vec<u8> {
    a.iter().flat_map(|v| v.to_bytes().to_vec()).collect()
}

fn sha(bits: u32, data: &[u8]) -> Vec<u8> {
    use sha2::Digest;
    match bits {
        1 => sha1::Sha1::digest(data).to_vec(),
        384 => sha2::Sha384::digest(data).to_vec(),
        512 => sha2::Sha512::digest(data).to_vec(),
        _ => sha2::Sha256::digest(data).to_vec(),
    }
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

/// `$fh->autoflush(N)` (IO::Handle): STDOUT unbuffered when set, as `$|`;
/// other handles are written at once here.
fn io_autoflush(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let on = a.get(1).map(|v| v.truthy()).unwrap_or(true);
    if let Some(Value::Handle(h)) = a.first() {
        if std::rc::Rc::ptr_eq(h, &it.bareword_handle("STDOUT").unwrap()) {
            let old = it.get_scalar("main::|");
            it.set_scalar("main::|", Value::Int(on as i64));
            return Ok(vec![old]);
        }
    }
    Ok(vec![Value::Int(0)])
}

pub fn register(it: &mut Interp) {
    it.define_native("IO::Handle::autoflush", io_autoflush);
    // $fh->flush: write out what the handle holds
    it.define_native("IO::Handle::flush", |it, _a, _w| {
        it.flush_handles();
        Ok(vec![Value::Int(1)])
    });
    it.set_global(b'@', "IO::File::ISA", crate::interp::Binding::Array(std::rc::Rc::new(std::cell::RefCell::new(vec![Value::str("IO::Handle")]))));
    it.set_global(b'@', "FileHandle::ISA", crate::interp::Binding::Array(std::rc::Rc::new(std::cell::RefCell::new(vec![Value::str("IO::File")]))));
    // Digest::MD5, functional and OO (the object keeps the data added).
    it.define_native("Digest::MD5::md5", |_, a, _| ret(Value::bytes(md5(&a.iter().flat_map(|v| v.to_bytes().to_vec()).collect::<Vec<u8>>()))));
    it.define_native("Digest::MD5::md5_hex", |_, a, _| ret(Value::str(&hex(&md5(&a.iter().flat_map(|v| v.to_bytes().to_vec()).collect::<Vec<u8>>())))));
    it.define_native("Digest::MD5::new", |_, _, _| {
        let h = Value::new_hash();
        if let Value::HashRef(r) = &h {
            r.borrow_mut().blessed = Some(std::rc::Rc::from("Digest::MD5"));
        }
        h.set("data", Value::str(""));
        ret(h)
    });
    it.define_native("Digest::MD5::add", |_, a, _| {
        let o = arg(&a, 0);
        let mut d = o.get("data").to_bytes().to_vec();
        for v in &a[1..] {
            d.extend_from_slice(&v.to_bytes());
        }
        o.set("data", Value::bytes(d));
        ret(o)
    });
    it.define_native("Digest::MD5::digest", |_, a, _| {
        let o = arg(&a, 0);
        let r = md5(&o.get("data").to_bytes());
        o.set("data", Value::str(""));
        ret(Value::bytes(r))
    });
    it.define_native("Digest::MD5::hexdigest", |_, a, _| {
        let o = arg(&a, 0);
        let r = md5(&o.get("data").to_bytes());
        o.set("data", Value::str(""));
        ret(Value::str(&hex(&r)))
    });
    // Digest::SHA: sha1/sha256/sha384/sha512 (+ _hex), and the OO interface.
    for (name, bits) in [("sha1", 1u32), ("sha256", 256), ("sha384", 384), ("sha512", 512)] {
        it.define_native(&format!("Digest::SHA::{name}"), move |_, a, _| ret(Value::bytes(sha(bits, &concat(&a)))));
        it.define_native(&format!("Digest::SHA::{name}_hex"), move |_, a, _| ret(Value::str(&hex(&sha(bits, &concat(&a))))));
    }
    it.define_native("Digest::SHA::new", |_, a, _| {
        let h = Value::new_hash();
        if let Value::HashRef(r) = &h {
            r.borrow_mut().blessed = Some(std::rc::Rc::from("Digest::SHA"));
        }
        let alg = a.get(1).map(|v| v.to_i64()).unwrap_or(1);
        h.set("alg", Value::Int(alg));
        h.set("data", Value::str(""));
        ret(h)
    });
    it.define_native("Digest::SHA::add", |_, a, _| {
        let o = arg(&a, 0);
        let mut d = o.get("data").to_bytes().to_vec();
        for v in &a[1..] {
            d.extend_from_slice(&v.to_bytes());
        }
        o.set("data", Value::bytes(d));
        ret(o)
    });
    it.define_native("Digest::SHA::digest", |_, a, _| {
        let o = arg(&a, 0);
        let r = sha(o.get("alg").to_i64() as u32, &o.get("data").to_bytes());
        o.set("data", Value::str(""));
        ret(Value::bytes(r))
    });
    it.define_native("Digest::SHA::hexdigest", |_, a, _| {
        let o = arg(&a, 0);
        let r = sha(o.get("alg").to_i64() as u32, &o.get("data").to_bytes());
        o.set("data", Value::str(""));
        ret(Value::str(&hex(&r)))
    });
    // overload: no Perl objects here overload operators.
    it.define_native("overload::Method", |_, _, _| ret(Value::Undef));
    it.define_native("overload::Overloaded", |_, _, _| ret(Value::from(false)));
    it.define_native("overload::StrVal", |_, a, _| ret(Value::str(&arg(&a, 0).to_string())));
    it.define_native("File::Basename::dirname", |_, a, _| ret(Value::bytes(dirname(&arg(&a, 0).to_bytes()))));
    it.define_native("File::Basename::basename", |_, a, _| {
        let sfx: Vec<Vec<u8>> = a.iter().skip(1).map(|v| v.to_bytes().to_vec()).collect();
        ret(Value::bytes(basename(&arg(&a, 0).to_bytes(), &sfx)))
    });
    it.define_native("File::Basename::fileparse", |_, a, want| {
        let sfx: Vec<Vec<u8>> = a.iter().skip(1).map(|v| v.to_bytes().to_vec()).collect();
        let (b, d, s) = fileparse(&arg(&a, 0).to_bytes(), &sfx, true);
        if want == Want::List {
            Ok(vec![Value::bytes(b), Value::bytes(d), Value::bytes(s)])
        } else {
            ret(Value::bytes(b))
        }
    });
    // Encode, on perl's character strings (Value::UStr): decoding makes
    // one, encoding gives bytes
    it.define_native("Encode::is_utf8", |_, a, _| ret(Value::from(arg(&a, 0).is_utf8())));
    it.define_native("Encode::encode", |_, a, _| {
        let v = arg(&a, 1);
        if v.is_undef() {
            return ret(Value::Undef);
        }
        ret(match encode_to(&arg(&a, 0).to_text(), &v) {
            Some(b) => Value::bytes(b),
            None => Value::bytes(v.to_bytes()),
        })
    });
    it.define_native("Encode::encode_utf8", |_, a, _| {
        let v = arg(&a, 0);
        ret(if v.is_undef() { Value::Undef } else { Value::bytes(v.utf8_bytes()) })
    });
    it.define_native("Encode::decode", |_, a, _| {
        let v = arg(&a, 1);
        if v.is_undef() {
            return ret(Value::Undef);
        }
        ret(match decode_from(&arg(&a, 0).to_text(), &v.to_bytes()) {
            Some(c) => Value::chars(c),
            None => Value::bytes(v.to_bytes()),
        })
    });
    it.define_native("Encode::decode_utf8", |_, a, _| {
        let v = arg(&a, 0);
        ret(if v.is_undef() { Value::Undef } else { Value::chars(crate::perl::utf8::decode_strict(&v.to_bytes())) })
    });
    it.define_native("File::Glob::bsd_glob", |_, a, _| Ok(crate::interp::eval::glob::bsd_glob(&arg(&a, 0).to_bytes()).into_iter().map(Value::bytes).collect()));
    it.define_native("File::Glob::csh_glob", |_, a, _| Ok(crate::interp::eval::glob::csh_glob(&arg(&a, 0).to_bytes()).into_iter().map(Value::bytes).collect()));
    it.define_native("Cwd::getcwd", |_, _, _| ret(Value::str(&std::env::current_dir().map(|p| p.to_string_lossy().into_owned()).unwrap_or_default())));
    it.define_native("Cwd::cwd", |_, _, _| ret(Value::str(&std::env::current_dir().map(|p| p.to_string_lossy().into_owned()).unwrap_or_default())));
    it.define_native("Cwd::abs_path", |_, a, _| {
        let p = arg(&a, 0).to_text();
        ret(match std::fs::canonicalize(if p.is_empty() { "." } else { &p }) {
            Ok(c) => Value::str(&c.to_string_lossy()),
            Err(_) => Value::Undef,
        })
    });
    // Time::Local: timelocal/timegm with perl's year rules (two-digit years
    // in a rolling century, 4-digit as is, years >= 1000 taken literally).
    it.define_native("Time::Local::timegm", |_, a, _| ret(num(timegm_value(&a, true)?)));
    it.define_native("Time::Local::timelocal", |it, a, _| {
        crate::interp::time::sync_tz(it);
        ret(num(timelocal_value(&a, true)?))
    });
    it.define_native("Time::Local::timegm_nocheck", |_, a, _| ret(num(timegm_value(&a, false)?)));
    it.define_native("Time::Local::timelocal_nocheck", |it, a, _| {
        crate::interp::time::sync_tz(it);
        ret(num(timelocal_value(&a, false)?))
    });
    it.define_native("POSIX::floor", |_, a, _| ret(Value::Num(arg(&a, 0).to_f64().floor())));
    it.define_native("POSIX::ceil", |_, a, _| ret(Value::Num(arg(&a, 0).to_f64().ceil())));
    it.define_native("POSIX::fmod", |_, a, _| ret(Value::Num(arg(&a, 0).to_f64() % arg(&a, 1).to_f64())));
    it.define_native("POSIX::tzset", |it, _, _| {
        crate::interp::time::sync_tz(it);
        #[cfg(unix)]
        unsafe {
            extern "C" {
                fn tzset();
            }
            // SAFETY: tzset reads TZ; no other thread changes it here.
            tzset();
        }
        Ok(vec![])
    });
    it.define_native("POSIX::strftime", |_, a, _| {
        let fmt = arg(&a, 0).to_text();
        let tm: Vec<i64> = a.iter().skip(1).map(|v| v.to_i64()).collect();
        ret(Value::str(&crate::interp::time::strftime(&fmt, &tm)))
    });
    it.define_native("POSIX::mktime", |it, a, _| {
        // mktime(sec, min, hour, mday, mon, year): year is tm_year (from
        // 1900) and out-of-range fields carry over, as the C function
        crate::interp::time::sync_tz(it);
        let g = |i: usize| a.get(i).map(|v| v.to_f64().trunc() as i64).unwrap_or(0);
        let months = g(5) * 12 + g(4);
        let (y, m) = (1900 + months.div_euclid(12), months.rem_euclid(12));
        let days = crate::interp::time::days_from_civil(y, m + 1, 1) + g(3) - 1;
        let t = (days * 86400 + g(2) * 3600 + g(1) * 60 + g(0)) as f64;
        ret(Value::Int(crate::interp::time::local_to_utc(t) as i64))
    });
    it.define_native("Scalar::Util::blessed", |_, a, _| {
        ret(match arg(&a, 0) {
            Value::HashRef(h) => match &h.borrow().blessed {
                Some(c) => Value::str(c),
                None => Value::Undef,
            },
            _ => Value::Undef,
        })
    });
    it.define_native("Scalar::Util::looks_like_number", |_, a, _| ret(Value::from(arg(&a, 0).looks_like_number())));
    it.define_native("Scalar::Util::refaddr", |_, a, _| {
        let v = arg(&a, 0);
        ret(if v.is_ref() { Value::Int(v.to_i64()) } else { Value::Undef })
    });
    it.define_native("Scalar::Util::reftype", |_, a, _| {
        let v = arg(&a, 0);
        ret(if v.is_ref() { Value::str(v.ref_type()) } else { Value::Undef })
    });
    // utf8::, on perl's character strings (Value::UStr); the ones that
    // change their argument write it back (`$_[0]`)
    it.define_native("utf8::is_utf8", |_, a, _| ret(Value::from(arg(&a, 0).is_utf8())));
    it.define_native("utf8::upgrade", |it, a, _| {
        let v = arg(&a, 0);
        if v.is_undef() {
            return ret(Value::Int(0));
        }
        let u = v.utf8_bytes();
        let n = u.len() as i64;
        if !v.is_utf8() {
            it.set_arg(0, Value::chars(u))?;
        }
        ret(Value::Int(n))
    });
    it.define_native("utf8::downgrade", |it, a, _| {
        let v = arg(&a, 0);
        let Value::UStr(s) = &v else { return ret(Value::Int(1)) };
        match crate::perl::utf8::downgrade(s) {
            Some(b) => {
                it.set_arg(0, Value::bytes(b))?;
                ret(Value::Int(1))
            }
            None if arg(&a, 1).truthy() => ret(Value::str("")),
            None => crate::interp::eval::die(format!("Wide character in subroutine entry{}", crate::interp::eval::location_suffix())),
        }
    });
    it.define_native("utf8::encode", |it, a, _| {
        let v = arg(&a, 0);
        if !v.is_undef() {
            it.set_arg(0, Value::bytes(v.utf8_bytes()))?;
        }
        Ok(vec![])
    });
    it.define_native("utf8::decode", |it, a, _| {
        let v = arg(&a, 0);
        // a character string is first made bytes again (not possible with
        // a character above 0xFF)
        let b = match &v {
            Value::UStr(s) => match crate::perl::utf8::downgrade(s) {
                Some(b) => b,
                None => return ret(Value::str("")),
            },
            _ => v.to_bytes().to_vec(),
        };
        if !crate::perl::utf8::is_lax_utf8(&b) {
            return ret(Value::str(""));
        }
        // (only a string with non-ASCII characters gets the flag)
        if !b.is_ascii() {
            it.set_arg(0, Value::chars(b))?;
        } else if v.is_utf8() {
            it.set_arg(0, Value::bytes(b))?;
        }
        ret(Value::Int(1))
    });
    it.define_native("utf8::valid", |_, a, _| {
        let v = arg(&a, 0);
        ret(Value::from(match &v {
            Value::UStr(s) => crate::perl::utf8::is_lax_utf8(s),
            _ => true,
        }))
    });
}

/// The name of an encoding as Encode resolves it (lower case, without
/// `-` and `_`).
fn encoding_key(name: &str) -> String {
    name.to_ascii_lowercase().chars().filter(|c| *c != '-' && *c != '_' && *c != ' ').collect()
}

/// `Encode::decode(NAME, BYTES)` for the encodings the port knows: the
/// characters' UTF-8 form; None for another encoding (the bytes are kept).
fn decode_from(name: &str, b: &[u8]) -> Option<Vec<u8>> {
    use crate::perl::utf8::{push_cp, upgrade};
    let units16 = |be: bool| -> Vec<u8> {
        let mut out = Vec::new();
        let u: Vec<u16> = b.chunks_exact(2).map(|c| if be { u16::from_be_bytes([c[0], c[1]]) } else { u16::from_le_bytes([c[0], c[1]]) }).collect();
        for r in char::decode_utf16(u) {
            push_cp(&mut out, r.map(|c| c as u32).unwrap_or(0xFFFD));
        }
        if b.len() % 2 == 1 {
            push_cp(&mut out, 0xFFFD);
        }
        out
    };
    let units32 = |be: bool| -> Vec<u8> {
        let mut out = Vec::new();
        for c in b.chunks(4) {
            let v = if c.len() < 4 { 0xFFFD } else if be { u32::from_be_bytes([c[0], c[1], c[2], c[3]]) } else { u32::from_le_bytes([c[0], c[1], c[2], c[3]]) };
            push_cp(&mut out, if v > 0x10FFFF { 0xFFFD } else { v });
        }
        out
    };
    Some(match encoding_key(name).as_str() {
        "utf8" | "utf8strict" => crate::perl::utf8::decode_strict(b),
        "latin1" | "iso88591" | "l1" => upgrade(b),
        "ascii" | "usascii" => {
            let mut out = Vec::new();
            for &c in b {
                push_cp(&mut out, if c < 0x80 { c as u32 } else { 0xFFFD });
            }
            out
        }
        "utf16be" | "ucs2be" => units16(true),
        "utf16le" | "ucs2le" => units16(false),
        "utf16" => match b {
            [0xFF, 0xFE, rest @ ..] => decode_from("UTF-16LE", rest)?,
            [0xFE, 0xFF, rest @ ..] => decode_from("UTF-16BE", rest)?,
            _ => units16(true),
        },
        "utf32be" | "ucs4be" => units32(true),
        "utf32le" | "ucs4le" => units32(false),
        _ => return None,
    })
}

/// `Encode::encode(NAME, STRING)` for the encodings the port knows (a
/// byte string is taken as its characters 0-255); None for another
/// encoding. Characters the encoding has not become `?`, as Encode's
/// default substitution.
fn encode_to(name: &str, v: &Value) -> Option<Vec<u8>> {
    let chars: Vec<u32> = match v {
        Value::UStr(s) => crate::perl::utf8::chars(s).collect(),
        _ => v.to_bytes().iter().map(|&c| c as u32).collect(),
    };
    let units16 = |be: bool| -> Vec<u8> {
        let mut out = Vec::new();
        for &c in &chars {
            let ch = char::from_u32(c).unwrap_or('\u{FFFD}');
            let mut buf = [0u16; 2];
            for u in ch.encode_utf16(&mut buf) {
                out.extend_from_slice(&if be { u.to_be_bytes() } else { u.to_le_bytes() });
            }
        }
        out
    };
    Some(match encoding_key(name).as_str() {
        "utf8" | "utf8strict" => v.utf8_bytes().to_vec(),
        "latin1" | "iso88591" | "l1" => chars.iter().map(|&c| if c < 256 { c as u8 } else { b'?' }).collect(),
        "ascii" | "usascii" => chars.iter().map(|&c| if c < 128 { c as u8 } else { b'?' }).collect(),
        "utf16be" | "ucs2be" => units16(true),
        "utf16le" | "ucs2le" => units16(false),
        "utf16" => {
            let mut out = vec![0xFE, 0xFF];
            out.extend(units16(true));
            out
        }
        "utf32be" | "ucs4be" => chars.iter().flat_map(|c| c.to_be_bytes()).collect(),
        "utf32le" | "ucs4le" => chars.iter().flat_map(|c| c.to_le_bytes()).collect(),
        _ => return None,
    })
}

/// Time::Local's _daygm: days since the epoch of (mday, month, tm_year),
/// with its arithmetic (perl's % and int on possibly fractional values).
fn daygm(mday: f64, month: f64, year: f64) -> f64 {
    let raw = |mday: f64, month: f64, year: f64| {
        let m = ((month + 10.0).trunc() as i64).rem_euclid(12) as f64;
        let y = year + 1900.0 - (m / 10.0).trunc();
        mday + (365.0 * y + (y / 4.0).trunc() - (y / 100.0).trunc() + (y / 400.0).trunc() + ((m * 306.0 + 5.0) / 10.0).trunc())
    };
    raw(mday, month, year) - raw(1.0, 0.0, 70.0)
}

/// Time::Local's _timegm on a localtime() list.
fn timegm_tm(tm: &[i64; 9]) -> f64 {
    tm[0] as f64 + 60.0 * tm[1] as f64 + 3600.0 * tm[2] as f64 + 86400.0 * daygm(tm[3] as f64, tm[4] as f64, tm[5] as f64)
}

fn num(t: f64) -> Value {
    if t.fract() == 0.0 && t.abs() < 9.0e15 { Value::Int(t as i64) } else { Value::Num(t) }
}

/// Time::Local's timegm (`check`: the range checks the _nocheck variants
/// skip, with its messages).
fn timegm_value(a: &[Value], check: bool) -> ER<f64> {
    let g = |i: usize| a.get(i).map(|v| v.to_f64()).unwrap_or(0.0);
    let (sec, min, hour, mday, mon) = (g(0), g(1), g(2), g(3), g(4));
    let fmt = |i: usize| a.get(i).map(|v| v.to_string()).unwrap_or_default();
    // 1000 and up is a year, 0..99 a year in the century within 50 years
    // of now, anything else an offset from 1900
    let mut year = g(5);
    if year >= 1000.0 {
        year -= 1900.0;
    } else if (0.0..100.0).contains(&year) {
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0);
        let this_year = crate::interp::time::broken_down(now, true)[5];
        let breakpoint = (this_year + 50) % 100;
        let mut next_century = this_year - this_year % 100;
        if breakpoint < 50 {
            next_century += 100;
        }
        let century = next_century - 100;
        year += if year > breakpoint as f64 { century as f64 } else { next_century as f64 };
    }
    if check {
        if mon > 11.0 || mon < 0.0 {
            return die(format!("Month '{}' out of range 0..11", fmt(4)));
        }
        const MONTH_DAYS: [i64; 12] = [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
        let mut md = MONTH_DAYS[mon as usize];
        let fy = (year + 1900.0) as i64;
        if mon as usize == 1 && fy % 4 == 0 && (fy % 100 != 0 || fy % 400 == 0) {
            md += 1;
        }
        if mday > md as f64 || mday < 1.0 {
            return die(format!("Day '{}' out of range 1..{md}", fmt(3)));
        }
        if hour > 23.0 || hour < 0.0 {
            return die(format!("Hour '{}' out of range 0..23", fmt(2)));
        }
        if min > 59.0 || min < 0.0 {
            return die(format!("Minute '{}' out of range 0..59", fmt(1)));
        }
        if sec >= 60.0 || sec < 0.0 {
            return die(format!("Second '{}' out of range 0..59", fmt(0)));
        }
    }
    Ok(sec + 60.0 * min + 3600.0 * hour + 86400.0 * daygm(mday, mon, year))
}

/// Time::Local's timelocal: timegm, then corrected by the local offset at
/// that time, with its DST adjustments (and its rounding of fractional
/// seconds through localtime).
fn timelocal_value(a: &[Value], check: bool) -> ER<f64> {
    let lt = |t: f64| crate::interp::time::broken_down(t.trunc() as i64, true);
    let ref_t = timegm_value(a, check)?;
    let loc_for_ref_t = timegm_tm(&lt(ref_t));
    let zone_off = loc_for_ref_t - ref_t;
    if zone_off == 0.0 {
        return Ok(loc_for_ref_t);
    }
    let mut loc_t = ref_t - zone_off;
    let dst_off = ref_t - timegm_tm(&lt(loc_t));
    if dst_off == 0.0 && (ref_t - 3600.0) - timegm_tm(&lt(loc_t - 3600.0)) < 0.0 {
        return Ok(loc_t - 3600.0);
    }
    loc_t += dst_off;
    if dst_off > 0.0 {
        return Ok(loc_t);
    }
    let tm = lt(loc_t);
    let g = |i: usize| a.get(i).map(|v| v.to_f64()).unwrap_or(0.0);
    if tm[0] as f64 != g(0) || tm[1] as f64 != g(1) || tm[2] as f64 != g(2) {
        loc_t -= dst_off;
    }
    Ok(loc_t)
}
