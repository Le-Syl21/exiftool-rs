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

pub fn register(it: &mut Interp) {
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
    it.define_native("Time::Local::timegm", |_, a, _| timegm_native(&a, false));
    it.define_native("Time::Local::timelocal", |_, a, _| timegm_native(&a, true));
    it.define_native("Time::Local::timegm_nocheck", |_, a, _| timegm_native(&a, false));
    it.define_native("Time::Local::timelocal_nocheck", |_, a, _| timegm_native(&a, true));
    it.define_native("POSIX::floor", |_, a, _| ret(Value::Num(arg(&a, 0).to_f64().floor())));
    it.define_native("POSIX::ceil", |_, a, _| ret(Value::Num(arg(&a, 0).to_f64().ceil())));
    it.define_native("POSIX::fmod", |_, a, _| ret(Value::Num(arg(&a, 0).to_f64() % arg(&a, 1).to_f64())));
    it.define_native("POSIX::tzset", |_, _, _| {
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
    it.define_native("POSIX::mktime", |_, a, _| timegm_native(&a, true));
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
    // utf8:: functions on byte strings: perl's character strings are not
    // modelled; byte strings are what ExifTool handles.
    it.define_native("utf8::is_utf8", |_, _, _| ret(Value::from(false)));
    it.define_native("utf8::upgrade", |_, a, _| ret(Value::Int(arg(&a, 0).to_bytes().len() as i64)));
    it.define_native("utf8::downgrade", |_, _, _| ret(Value::Int(1)));
    it.define_native("utf8::encode", |_, _, _| Ok(vec![]));
    it.define_native("utf8::decode", |_, a, _| ret(Value::from(std::str::from_utf8(&arg(&a, 0).to_bytes()).is_ok())));
    it.define_native("utf8::valid", |_, a, _| ret(Value::from(std::str::from_utf8(&arg(&a, 0).to_bytes()).is_ok())));
}

fn timegm_native(a: &[Value], local: bool) -> ER<Vec<Value>> {
    let g = |i: usize| a.get(i).map(|v| v.to_f64()).unwrap_or(0.0);
    let (sec, min, hour, mday, mon) = (g(0), g(1) as i64, g(2) as i64, g(3) as i64, g(4) as i64);
    let mut year = g(5) as i64;
    // Time::Local's year interpretation.
    if year >= 1000 {
    } else if (100..1000).contains(&year) {
        year += 1900;
    } else {
        // 0..99: rolling century, within 50 years of now.
        let now_year = 1900 + crate::interp::time::broken_down(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0), false)[5];
        let century = now_year / 100 * 100;
        year += century;
        if year > now_year + 50 {
            year -= 100;
        } else if year <= now_year - 50 {
            year += 100;
        }
    }
    if !(0..=11).contains(&mon) {
        return die(format!("Month '{mon}' out of range 0..11"));
    }
    let days = crate::interp::time::days_from_civil(year, mon + 1, mday);
    let mut t = days as f64 * 86400.0 + (hour * 3600 + min * 60) as f64 + sec;
    if local {
        t = crate::interp::time::local_to_utc(t);
    }
    Ok(vec![if t.fract() == 0.0 { Value::Int(t as i64) } else { Value::Num(t) }])
}
