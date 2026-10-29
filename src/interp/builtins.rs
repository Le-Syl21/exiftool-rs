//! Perl's builtin functions, as ExifTool uses them.
//!
//! Builtins receive their argument expressions, not values: `keys %h`,
//! `push @a`, `delete $h{k}` and `defined &name` act on the storage the
//! expression names.

use std::cell::RefCell;
use std::rc::Rc;

use super::*;
use crate::perl::{pack, Number};

type Out = Option<ER<Vec<Value>>>;

fn one(v: Value) -> ER<Vec<Value>> {
    Ok(vec![v])
}

/// Run builtin `name`, or `None` when it is not one.
/// Run builtin `name`, or `None` when it is not one.
pub(super) fn call(it: &mut Interp, name: &str, args: &[Expr], scope: &Rc<Scope>, want: Want) -> Out {
    if !BUILTINS.contains(&name) {
        return None;
    }
    Some(run(it, name, args, scope, want))
}

const BUILTINS: &[&str] = &["utime", "chmod", "readpipe", "getpwuid", "getgrgid", "defined", "ref", "scalar", "lc", "uc", "lcfirst", "ucfirst", "quotemeta", "fc", "length", "chr", "ord", "hex", "oct", "abs", "int", "sqrt", "log", "exp", "sin", "cos", "atan2", "rand", "srand", "time", "localtime", "gmtime", "join", "sprintf", "reverse", "split", "pack", "unpack", "substr", "index", "rindex", "keys", "values", "each", "exists", "delete", "shift", "pop", "push", "unshift", "splice", "chomp", "chop", "undef", "pos", "die", "warn", "bless", "require", "caller", "lock", "study", "exit", "sleep", "do", "open", "close", "binmode", "unlink", "mkdir", "rmdir", "opendir", "readdir", "closedir", "system", "exec", "select", "seek", "read", "sysread", "eof", "readline", "fileno", "chdir", "rename", "link", "symlink", "stat", "lstat", "glob"];

fn run(it: &mut Interp, name: &str, args: &[Expr], scope: &Rc<Scope>, want: Want) -> ER<Vec<Value>> {
    match name {
        "defined" => defined(it, args, scope),
        "ref" => {
            let v = arg_or_topic(it, args, scope)?;
            let r = match &v {
                Value::ScalarRef(c) => {
                    if c.borrow().is_ref() {
                        "REF"
                    } else {
                        "SCALAR"
                    }
                }
                Value::ArrayRef(_) => "ARRAY",
                Value::HashRef(h) => {
                    if let Some(c) = &h.borrow().blessed {
                        return one(Value::str(c));
                    }
                    "HASH"
                }
                Value::Code(_) => "CODE",
                Value::Handle(h) => {
                    if let Some(c) = &h.borrow().class {
                        return one(Value::str(c));
                    }
                    "GLOB"
                }
                Value::LvRef(l) => {
                    if l.get().is_ref() {
                        "REF"
                    } else {
                        "SCALAR"
                    }
                }
                _ => "",
            };
            one(Value::str(r))
        }
        "scalar" => match args.first() {
            Some(a) => it.scalar(a, scope).map(|v| vec![v]),
            None => die("Not enough arguments for scalar"),
        },
        "lc" | "uc" | "lcfirst" | "ucfirst" | "quotemeta" | "fc" => {
            let v = it.arg_used(args, scope, name, false)?;
            if v.is_undef() && name != "quotemeta" {
                return one(Value::str(""));
            }
            let (b, utf8) = v.str_form();
            use crate::perl::utf8::{change_case, Case};
            let out: Vec<u8> = match name {
                "lc" => change_case(&b, utf8, Case::Lower),
                "fc" => change_case(&b, utf8, Case::Fold),
                "uc" => change_case(&b, utf8, Case::Upper),
                "lcfirst" => change_case(&b, utf8, Case::LowerFirst),
                "ucfirst" => change_case(&b, utf8, Case::UpperFirst),
                _ if utf8 => crate::perl::regex::quotemeta_chars(&b),
                _ => crate::perl::regex::quotemeta(&b),
            };
            one(Value::string(out, utf8))
        }
        "length" => {
            let v = arg_or_topic(it, args, scope)?;
            one(crate::pl::length(&v))
        }
        "chr" => {
            let v = it.arg_used(args, scope, "chr", true)?;
            let n = v.to_f64();
            let n = if n < 0.0 || n.is_nan() { 0xFFFD } else { n as u32 };
            if n < 256 {
                return one(Value::bytes([n as u8]));
            }
            let mut out = Vec::new();
            crate::perl::utf8::push_cp(&mut out, n);
            one(Value::chars(out))
        }
        "ord" => {
            let v = it.arg_used(args, scope, "ord", false)?;
            one(Value::Int(match &v {
                Value::UStr(s) if !s.is_empty() => crate::perl::utf8::decode_at(s, 0).0 as i64,
                _ => v.to_bytes().first().copied().unwrap_or(0) as i64,
            }))
        }
        "hex" => {
            let v = it.arg_used(args, scope, "hex", false)?;
            one(parse_radix(&v.to_bytes(), 16))
        }
        "oct" => {
            let v = it.arg_used(args, scope, "oct", false)?;
            one(oct(&v.to_bytes()))
        }
        "abs" => it.arg_used(args, scope, "abs", true).map(|v| vec![crate::perl::ops::abs(&v)]),
        "int" => it.arg_used(args, scope, "int", true).map(|v| vec![crate::perl::ops::int(&v)]),
        "sqrt" => {
            let v = it.arg_used(args, scope, "sqrt", true)?.to_f64();
            if v < 0.0 {
                return die(format!("Can't take sqrt of {}", crate::perl::sprintf::format_g(v, 6, false, false)));
            }
            one(Value::Num(v.sqrt()))
        }
        "log" => {
            let v = it.arg_used(args, scope, "log", true)?.to_f64();
            if v <= 0.0 {
                return die(format!("Can't take log of {}", crate::perl::sprintf::format_g(v, 6, false, false)));
            }
            one(Value::Num(v.ln()))
        }
        "exp" => it.arg_used(args, scope, "exp", true).map(|v| vec![Value::Num(v.to_f64().exp())]),
        "sin" => it.arg_used(args, scope, "sin", true).map(|v| vec![Value::Num(v.to_f64().sin())]),
        "cos" => it.arg_used(args, scope, "cos", true).map(|v| vec![Value::Num(v.to_f64().cos())]),
        "atan2" => {
            let (a, origin) = it.values_traced(args, scope)?;
            for (j, v) in a.iter().enumerate().take(2) {
                it.list_use(v, j, args, &origin, scope, "atan2", true)?;
            }
            let y = a.first().map(|v| v.to_f64()).unwrap_or(0.0);
            let x = a.get(1).map(|v| v.to_f64()).unwrap_or(0.0);
            one(Value::Num(y.atan2(x)))
        }
        "rand" => {
            let m = match args.first() {
                Some(a) => it.scalar(a, scope)?.to_f64(),
                None => 1.0,
            };
            let m = if m == 0.0 { 1.0 } else { m };
            one(Value::Num(pseudo_random() * m))
        }
        "srand" => {
            let seed = match args.first() {
                Some(a) => {
                    let v = it.scalar(a, scope)?;
                    match srand_seed(&v) {
                        Some(n) => n,
                        None => {
                            crate::pl::warn(it, "Integer overflow in srand")?;
                            u64::MAX
                        }
                    }
                }
                None => auto_seed() as u64,
            };
            seed_rand48(seed as u32);
            // srand returns its seed ("0 but true" for 0)
            one(if seed == 0 { Value::str("0 but true") } else { crate::perl::ops::from_uv(seed) })
        }
        "time" => {
            let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
            one(Value::Int(t as i64))
        }
        "localtime" | "gmtime" => {
            let t = match args.first() {
                Some(a) => it.scalar(a, scope)?.to_f64(),
                None => std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs_f64()).unwrap_or(0.0),
            };
            if name == "localtime" {
                crate::interp::time::sync_tz(it);
            }
            let tm = crate::interp::time::broken_down(t.floor() as i64, name == "localtime");
            if want == Want::List {
                Ok(tm.iter().map(|&x| Value::Int(x)).collect())
            } else {
                one(Value::str(&crate::interp::time::ctime(&tm)))
            }
        }
        "join" => {
            let (vals, origin) = it.values_traced(args, scope)?;
            if !origin.is_empty() {
                // the separator is read when it joins something, or alone
                for (j, v) in vals.iter().enumerate() {
                    if v.is_undef() && (j > 0 || vals.len() != 2) {
                        it.list_use(v, j, args, &origin, scope, "join or string", false)?;
                    }
                }
            }
            let Some(sep) = vals.first() else { return one(Value::str("")) };
            let mut out = crate::perl::utf8::StrBuf::new();
            for (k, v) in vals[1..].iter().enumerate() {
                if k > 0 {
                    out.push_value(sep);
                }
                out.push_value(v);
            }
            one(out.into_value())
        }
        "sprintf" => {
            let (vals, origin) = it.values_traced(args, scope)?;
            one(it.sprintf_warned(&vals, args, &origin, scope, "sprintf")?)
        }
        "reverse" => {
            let vals = if args.is_empty() && want != Want::List { vec![it.read_scalar("_", scope)] } else { values(it, args, scope)? };
            if want == Want::List {
                Ok(vals.into_iter().rev().collect())
            } else {
                let mut s = crate::perl::utf8::StrBuf::new();
                for v in &vals {
                    if v.is_undef() {
                        let e = args.first().cloned().unwrap_or_else(|| Expr::Scalar(Rc::from("_")));
                        it.use_str(v, &e, scope, "reverse", false)?;
                    }
                    s.push_value(v);
                }
                if s.utf8 {
                    return one(Value::chars(crate::perl::utf8::reverse_chars(&s.buf)));
                }
                s.buf.reverse();
                one(Value::bytes(s.buf))
            }
        }
        "split" => split(it, args, scope, want),
        "pack" => {
            let (vals, origin) = it.values_traced(args, scope)?;
            if !origin.is_empty() {
                for (j, v) in vals.iter().enumerate() {
                    if v.is_undef() {
                        it.list_use(v, j, args, &origin, scope, "pack", false)?;
                    }
                }
            }
            let t = vals.first().map(|v| v.to_bytes()).unwrap_or_else(|| Rc::from(&b""[..]));
            match pack::pack(&t, &vals[1.min(vals.len())..]) {
                // a template that starts with U packs a character string
                Ok(b) => one(Value::string(b, pack::starts_with_u(&t))),
                Err(e) => die(e),
            }
        }
        "unpack" => {
            let t = match args.first() {
                Some(a) => it.scalar(a, scope)?.to_bytes(),
                None => return die("Not enough arguments for unpack"),
            };
            let d = it.arg_used(&args[1..], scope, "unpack", false)?;
            let r = match &d {
                Value::UStr(s) => pack::unpack_chars(&t, s),
                _ => pack::unpack(&t, &d.to_bytes()),
            };
            for w in pack::take_warnings() {
                crate::pl::warn(it, &w)?;
            }
            match r {
                // In scalar context, unpack returns the first value.
                Ok(v) if want != Want::List => one(v.into_iter().next().unwrap_or_default()),
                Ok(v) => Ok(v),
                Err(e) => die(e),
            }
        }
        "substr" => substr(it, args, scope),
        "index" | "rindex" => {
            let (a, origin) = it.values_traced(args, scope)?;
            if !origin.is_empty() {
                for (j, v) in a.iter().enumerate().take(3) {
                    it.list_use(v, j, args, &origin, scope, name, j == 2)?;
                }
            }
            let (sv, subv) = (a.first().cloned().unwrap_or_default(), a.get(1).cloned().unwrap_or_default());
            let pos = a.get(2).map(|v| v.to_i64());
            if sv.is_utf8() || subv.is_utf8() {
                // on characters: the UTF-8 forms, offsets converted
                let (s, sub) = (sv.utf8_bytes(), subv.utf8_bytes());
                let pos = pos.map(|p| if p <= 0 { p } else { crate::perl::utf8::byte_offset(&s, p as usize) as i64 });
                let r = if name == "index" { index_of(&s, &sub, pos) } else { rindex_of(&s, &sub, pos) };
                return one(Value::Int(if r < 0 { r } else { crate::perl::utf8::char_offset(&s, r as usize) as i64 }));
            }
            let (s, sub) = (sv.to_bytes(), subv.to_bytes());
            one(Value::Int(if name == "index" { index_of(&s, &sub, pos) } else { rindex_of(&s, &sub, pos) }))
        }
        "keys" | "values" => {
            let Some(a) = args.first() else { return die(format!("Not enough arguments for {name}")) };
            let pairs: Vec<(Value, Value)> = match a {
                Expr::Hash(c) => {
                    let h = it.hash_of(c, scope, true)?;
                    let h = h.borrow();
                    h.iter().map(|(k, v)| (h.key_value(k), v.clone())).collect()
                }
                Expr::Array(c) => {
                    let ar = it.array_of(c, scope, true)?;
                    let ar = ar.borrow();
                    ar.iter().enumerate().map(|(i, v)| (Value::Int(i as i64), v.clone())).collect()
                }
                e => match it.scalar(e, scope)? {
                    Value::HashRef(h) => {
                        let h = h.borrow();
                        h.iter().map(|(k, v)| (h.key_value(k), v.clone())).collect()
                    }
                    Value::ArrayRef(ar) => ar.borrow().iter().enumerate().map(|(i, v)| (Value::Int(i as i64), v.clone())).collect(),
                    _ => return die(format!("Type of argument to {name} must be hash or array")),
                },
            };
            if want != Want::List {
                return one(Value::Int(pairs.len() as i64));
            }
            Ok(pairs.into_iter().map(|(k, v)| if name == "keys" { k } else { v }).collect())
        }
        "each" => die("each is not supported"),
        "exists" => exists(it, args, scope),
        "delete" => delete(it, args, scope),
        "shift" | "pop" => {
            let a = match args.first() {
                Some(Expr::Array(c)) => it.array_of(c, scope, true)?,
                Some(e) => match it.scalar(e, scope)? {
                    Value::ArrayRef(a) => a,
                    _ => return die(format!("Type of arg 1 to {name} must be array")),
                },
                // @_ in a sub, @ARGV at file level.
                None => match scope.lookup(b'@', "_") {
                    Some(Binding::Array(a)) => a,
                    _ => match it.global(b'@', "main::ARGV") {
                        Binding::Array(a) => a,
                        _ => unreachable!(),
                    },
                },
            };
            let v = {
                let mut ab = a.borrow_mut();
                if name == "shift" {
                    if ab.is_empty() {
                        Value::Undef
                    } else {
                        ab.remove(0)
                    }
                } else {
                    ab.pop().unwrap_or_default()
                }
            };
            it.alias_shift(&a, name == "shift");
            one(v)
        }
        "push" | "unshift" => {
            let Some(first) = args.first() else { return die(format!("Not enough arguments for {name}")) };
            let a = match first {
                Expr::Array(c) => it.array_of(c, scope, true)?,
                e => match it.scalar(e, scope)? {
                    Value::ArrayRef(a) => a,
                    _ => return die(format!("Type of arg 1 to {name} must be array")),
                },
            };
            let vals = values(it, &args[1..], scope)?;
            if name == "unshift" {
                it.alias_forget(&a);
            }
            let mut a = a.borrow_mut();
            if name == "push" {
                a.extend(vals);
            } else {
                let tail = std::mem::take(&mut *a);
                *a = vals;
                a.extend(tail);
            }
            one(Value::Int(a.len() as i64))
        }
        "splice" => {
            let Some(Expr::Array(c)) = args.first() else { return die("Type of arg 1 to splice must be array") };
            let a = it.array_of(c, scope, true)?;
            let rest = values(it, &args[1..], scope)?;
            it.alias_forget(&a);
            let mut a = a.borrow_mut();
            let len = a.len() as i64;
            let off = rest.first().map(|v| v.to_i64()).unwrap_or(0);
            let off = if off < 0 { (len + off).max(0) } else { off.min(len) } as usize;
            let n = match rest.get(1) {
                Some(v) => {
                    let n = v.to_i64();
                    if n < 0 {
                        (len - off as i64 + n).max(0) as usize
                    } else {
                        (n as usize).min(a.len() - off)
                    }
                }
                None => a.len() - off,
            };
            let ins: Vec<Value> = rest.into_iter().skip(2).collect();
            let removed: Vec<Value> = a.splice(off..off + n, ins).collect();
            if want == Want::List {
                Ok(removed)
            } else {
                one(removed.last().cloned().unwrap_or_default())
            }
        }
        "chomp" | "chop" => {
            let targets: Vec<Lv> = if args.is_empty() { vec![Lv::Scalar(it.scalar_cell(scope, "_"))] } else { args.iter().map(|a| it.lvalue(a, scope)).collect::<ER<_>>()? };
            let mut count = 0;
            let mut last = Value::str("");
            // chomp removes $/ (all trailing newlines in paragraph mode).
            let sep = it.get_scalar("main::/");
            for (n, lv) in flatten_lv(targets).into_iter().enumerate() {
                let v = lv.get();
                if v.is_undef() {
                    if it.warn_on() {
                        let e = if args.len() == 1 && n == 0 { args[0].clone() } else if args.is_empty() { Expr::Scalar(Rc::from("_")) } else { Expr::Const(Value::Undef) };
                        let nm = it.uninit_name(&e, scope, false);
                        it.report_uninit(nm, if name == "chomp" { "scalar chomp" } else { "scalar chop" })?;
                    }
                    continue;
                }
                let (s, utf8) = v.str_form();
                if name == "chomp" {
                    if sep.is_undef() {
                        continue;
                    }
                    let sp = if utf8 { sep.utf8_bytes() } else { sep.to_bytes() };
                    if sp.is_empty() {
                        let n = s.iter().rev().take_while(|&&c| c == b'\n').count();
                        if n > 0 {
                            lv.set(Value::string(&s[..s.len() - n], utf8))?;
                            count += n as i64;
                        }
                    } else if s.ends_with(&sp) {
                        lv.set(Value::string(&s[..s.len() - sp.len()], utf8))?;
                        count += if utf8 { crate::perl::utf8::count(&sp) } else { sp.len() } as i64;
                    }
                } else if !s.is_empty() {
                    // the last character
                    let mut k = s.len() - 1;
                    while utf8 && k > 0 && (s[k] & 0xC0) == 0x80 {
                        k -= 1;
                    }
                    last = Value::string(&s[k..], utf8);
                    lv.set(Value::string(&s[..k], utf8))?;
                }
            }
            one(if name == "chomp" { Value::Int(count) } else { last })
        }
        "undef" => {
            match args.first() {
                None => {}
                Some(Expr::Array(c)) => it.array_of(c, scope, true)?.borrow_mut().clear(),
                Some(Expr::Hash(c)) => it.hash_of(c, scope, true)?.borrow_mut().clear(),
                Some(Expr::Call { name, amp: true, .. }) => {
                    let k = it.sub_key(name);
                    it.subs.remove(&*k);
                }
                Some(e) => it.lvalue(e, scope)?.set(Value::Undef)?,
            }
            one(Value::Undef)
        }
        "pos" => {
            let lv = match args.first() {
                Some(e) => it.lvalue(e, scope)?,
                None => Lv::Scalar(it.scalar_cell(scope, "_")),
            };
            one(match pos_key(&lv).and_then(|k| it.pos_at(k, &lv)) {
                // (kept as a byte offset in the string's form)
                Some(p) => Value::Int(match lv.get() {
                    Value::UStr(s) => crate::perl::utf8::char_offset(&s, p),
                    _ => p,
                } as i64),
                None => Value::Undef,
            })
        }
        "die" => {
            let vals = values(it, args, scope)?;
            if vals.len() == 1 && vals[0].is_ref() {
                return Err(Flow::Die(vals[0].clone()));
            }
            let mut msg: Vec<u8> = vals.iter().flat_map(|v| v.to_bytes().to_vec()).collect();
            if msg.is_empty() {
                msg = b"Died".to_vec();
            }
            if msg.last() != Some(&b'\n') {
                msg.extend_from_slice(super::location_suffix().as_bytes());
            }
            Err(Flow::Die(Value::bytes(msg)))
        }
        "warn" => {
            let vals = values(it, args, scope)?;
            let mut msg = crate::perl::utf8::StrBuf::new();
            vals.iter().for_each(|v| msg.push_value(v));
            if msg.buf.is_empty() {
                msg.push_bytes(b"Warning: something's wrong");
            }
            if msg.buf.last() != Some(&b'\n') {
                msg.push_bytes(super::location_suffix().as_bytes());
            }
            warn(it, msg.into_value())?;
            one(Value::Int(1))
        }
        "bless" => {
            let vals = values(it, args, scope)?;
            let class: Rc<str> = match vals.get(1) {
                Some(c) => Rc::from(c.to_text()),
                None => it.package.clone(),
            };
            match vals.first() {
                Some(Value::HashRef(h)) => {
                    h.borrow_mut().blessed = Some(class);
                    one(Value::HashRef(h.clone()))
                }
                Some(Value::Handle(h)) => {
                    h.borrow_mut().class = Some(class);
                    one(Value::Handle(h.clone()))
                }
                _ => die("Can't bless non-hash reference"),
            }
        }
        "require" => {
            match args.first() {
                Some(Expr::Const(v)) if v.looks_like_number() => {}
                Some(Expr::Call { name, args: a, amp: true }) if a.is_empty() || matches!(a.as_slice(), [Expr::Array(_)]) => it.require(name)?,
                Some(e) => {
                    let m = it.scalar(e, scope)?.to_text();
                    it.require(&m)?
                }
                None => {}
            }
            one(Value::Int(1))
        }
        "caller" => {
            // caller(N) adds the sub's name; outside any sub, nothing
            let level = match args.first() {
                Some(e) => Some(it.scalar(e, scope)?.to_i64().max(0) as usize),
                None => None,
            };
            match crate::interp::eval::caller_frame(level.unwrap_or(0)) {
                None => Ok(if want == Want::List { vec![] } else { vec![Value::Undef] }),
                Some((pkg, file, line, sub)) => {
                    if want != Want::List {
                        return one(Value::str(&pkg));
                    }
                    let file = file.map(|f| f.to_string()).unwrap_or_else(|| "-".to_string());
                    let mut v = vec![Value::str(&pkg), Value::str(&file), Value::Int(line as i64)];
                    if level.is_some() {
                        v.push(Value::str(&sub));
                    }
                    Ok(v)
                }
            }
        }
        "lock" | "study" => one(Value::Int(1)),
        "readpipe" => {
            let cmd = arg_or_topic(it, args, scope)?.to_text();
            // perl flushes its output handles before starting a child, and
            // the child's standard error is the program's
            it.flush_handles();
            let mut c = std::process::Command::new("sh");
            c.arg("-c").arg(&cmd).stdout(std::process::Stdio::piped());
            c.stderr(if it.passthrough { std::process::Stdio::inherit() } else { std::process::Stdio::piped() });
            let out = c.output();
            match out {
                Ok(o) => {
                    if !it.passthrough {
                        it.stderr.extend_from_slice(&o.stderr);
                    }
                    it.set_scalar("main::?", Value::Int(o.status.code().map(|c| (c as i64) << 8).unwrap_or(-1)));
                    if want == Want::List {
                        let mut lines = Vec::new();
                        for l in o.stdout.split_inclusive(|&b| b == b'\n') {
                            lines.push(Value::bytes(l));
                        }
                        Ok(lines)
                    } else {
                        one(Value::bytes(o.stdout))
                    }
                }
                Err(_) => one(Value::Undef),
            }
        }
        "getpwuid" | "getgrgid" => {
            let id = arg_or_topic(it, args, scope)?.to_i64();
            one(match crate::interp::time::user_or_group_name(id, name == "getpwuid") {
                Some(n) => Value::str(&n),
                None => Value::Undef,
            })
        }
        "exit" => {
            let code = match args.first() {
                Some(a) => it.scalar(a, scope)?.to_i64() as i32,
                None => 0,
            };
            Err(Flow::Exit(code))
        }
        "sleep" => one(Value::Int(0)),
        "rename" => {
            let a = values(it, args, scope)?;
            let ok = std::fs::rename(arg_text(&a, 0), arg_text(&a, 1)).is_ok();
            one(Value::from(ok))
        }
        // link OLD, NEW and symlink OLD, NEW: true on success, with $! set
        // otherwise
        "link" | "symlink" => {
            let a = values(it, args, scope)?;
            let (old, new) = (arg_text(&a, 0), arg_text(&a, 1));
            #[cfg(unix)]
            let r = if name == "link" { std::fs::hard_link(&old, &new) } else { std::os::unix::fs::symlink(&old, &new) };
            #[cfg(not(unix))]
            let r = if name == "link" { std::fs::hard_link(&old, &new) } else { Err(std::io::Error::other("symlink not supported")) };
            match r {
                Ok(()) => one(Value::Int(1)),
                Err(e) => {
                    it.set_scalar("main::!", Value::from(e.to_string()));
                    one(Value::Int(0))
                }
            }
        }
        "mkdir" => {
            let a = values(it, args, scope)?;
            let p = if a.is_empty() { it.read_scalar("_", scope).to_text() } else { arg_text(&a, 0) };
            let ok = std::fs::create_dir(&p).is_ok();
            #[cfg(unix)]
            if ok {
                use std::os::unix::fs::PermissionsExt;
                let mode = a.get(1).map(|v| v.to_i64() as u32).unwrap_or(0o777);
                let _ = std::fs::set_permissions(&p, std::fs::Permissions::from_mode(mode & !0o022));
            }
            one(Value::from(ok))
        }
        "glob" => {
            let p = arg_or_topic(it, args, scope)?.to_bytes();
            let list: Vec<Value> = super::glob::csh_glob(&p).into_iter().map(Value::bytes).collect();
            Ok(list)
        }
        "rmdir" => {
            let p = arg_or_topic(it, args, scope)?.to_text();
            one(Value::from(std::fs::remove_dir(&p).is_ok()))
        }
        "chdir" => {
            let p = arg_or_topic(it, args, scope)?.to_text();
            one(Value::from(std::env::set_current_dir(&p).is_ok()))
        }
        "opendir" => {
            // opendir(DH, $dir): the entries are kept in a handle-like hash.
            let Some(first) = args.first() else { return die("Not enough arguments for opendir") };
            let dir = match args.get(1) {
                Some(d) => it.scalar(d, scope)?.to_text(),
                None => return die("Not enough arguments for opendir"),
            };
            let entries = match std::fs::read_dir(&dir) {
                Ok(rd) => {
                    let mut v = vec![Value::str("."), Value::str("..")];
                    for e in rd.flatten() {
                        v.push(Value::str(&e.file_name().to_string_lossy()));
                    }
                    v
                }
                Err(_) => return one(Value::from(false)),
            };
            let key = match first {
                Expr::Const(c) => c.to_text(),
                e => {
                    let lv = it.lvalue(e, scope)?;
                    let k = format!("*dir{}", it.dir_handles.len());
                    lv.set(Value::str(&k))?;
                    k
                }
            };
            it.dir_handles.insert(key, (entries, 0));
            one(Value::from(true))
        }
        "readdir" => {
            let key = match args.first() {
                Some(Expr::Const(c)) => c.to_text(),
                Some(e) => it.scalar(e, scope)?.to_text(),
                None => return die("Not enough arguments for readdir"),
            };
            let Some((entries, pos)) = it.dir_handles.get_mut(&key) else { return one(Value::Undef) };
            if want == Want::List {
                let rest = entries[*pos..].to_vec();
                *pos = entries.len();
                Ok(rest)
            } else {
                let v = entries.get(*pos).cloned().unwrap_or_default();
                *pos += 1;
                one(v)
            }
        }
        "closedir" => {
            let key = match args.first() {
                Some(Expr::Const(c)) => c.to_text(),
                Some(e) => it.scalar(e, scope)?.to_text(),
                None => String::new(),
            };
            one(Value::from(it.dir_handles.remove(&key).is_some()))
        }
        "system" => {
            let a = values(it, args, scope)?;
            it.flush_handles();
            let status = if a.len() == 1 {
                std::process::Command::new("sh").arg("-c").arg(arg_text(&a, 0)).status()
            } else {
                std::process::Command::new(arg_text(&a, 0)).args(a[1..].iter().map(|v| v.to_text())).status()
            };
            let code = status.map(|s| s.code().unwrap_or(-1) << 8).unwrap_or(-1);
            it.set_scalar("main::?", Value::Int(code as i64));
            one(Value::Int(code as i64))
        }
        "utime" => {
            let a = values(it, args, scope)?;
            let (at, mt) = (a.first().map(|v| v.to_f64()), a.get(1).map(|v| v.to_f64()));
            let mut n = 0;
            for f in a.iter().skip(2) {
                if crate::interp::time::set_file_times(&f.to_text(), at, mt) {
                    n += 1;
                }
            }
            one(Value::Int(n))
        }
        "chmod" => {
            let a = values(it, args, scope)?;
            #[cfg_attr(not(unix), allow(unused_mut))]
            let mut n = 0;
            #[cfg(not(unix))]
            let _ = &a;
            #[cfg(unix)]
            for f in a.iter().skip(1) {
                use std::os::unix::fs::PermissionsExt;
                if std::fs::set_permissions(f.to_text(), std::fs::Permissions::from_mode(a[0].to_i64() as u32)).is_ok() {
                    n += 1;
                }
            }
            one(Value::Int(n))
        }
        "do" | "exec" | "readline" => {
            die(format!("{name} is not available in this context"))
        }
        _ => unreachable!(),
    }
}

fn arg_text(a: &[Value], i: usize) -> String {
    a.get(i).map(|v| v.to_text()).unwrap_or_default()
}

fn flatten_lv(v: Vec<Lv>) -> Vec<Lv> {
    let mut out = Vec::new();
    for lv in v {
        match lv {
            Lv::List(items) => out.extend(flatten_lv(items)),
            Lv::Array(a) => {
                let n = a.borrow().len();
                for i in 0..n {
                    out.push(Lv::AElem(a.clone(), i as i64));
                }
            }
            other => out.push(other),
        }
    }
    out
}

pub(super) fn warn(it: &mut Interp, msg: Value) -> ER<()> {
    let handler = match it.global(b'%', "main::SIG") {
        Binding::Hash(h) => h.borrow().get(b"__WARN__").cloned(),
        _ => None,
    };
    match handler {
        Some(Value::Code(c)) => {
            it.call(&c, vec![msg], Want::Void)?;
        }
        // ('IGNORE' does not apply to __WARN__: perl prints the warning)
        Some(Value::Str(s) | Value::UStr(s)) if &s[..] != b"DEFAULT" && &s[..] != b"IGNORE" && !s.is_empty() => {
            let n = String::from_utf8_lossy(&s).into_owned();
            it.call_named(&n, vec![msg], Want::Void)?;
        }
        _ => {
            let h = it.handle_of(&Value::str("*STDERR"), false).unwrap();
            let layer = h.borrow().utf8;
            let mut out = Vec::new();
            it.output_form(&mut out, &msg, layer, "warn")?;
            it.write_handle(&h, &out);
        }
    }
    Ok(())
}

/// The argument, or `$_` when there is none.
fn arg_or_topic(it: &mut Interp, args: &[Expr], scope: &Rc<Scope>) -> ER<Value> {
    match args.first() {
        Some(a) => it.scalar(a, scope),
        None => Ok(it.read_scalar("_", scope)),
    }
}

/// All arguments, flattened in list context.
fn values(it: &mut Interp, args: &[Expr], scope: &Rc<Scope>) -> ER<Vec<Value>> {
    let mut out = Vec::new();
    for a in args {
        out.extend(it.list(a, scope)?);
    }
    Ok(out)
}

fn defined(it: &mut Interp, args: &[Expr], scope: &Rc<Scope>) -> ER<Vec<Value>> {
    let b = match args.first() {
        None => !it.read_scalar("_", scope).is_undef(),
        Some(Expr::Call { name, amp: true, .. }) => {
            let k = it.sub_key(name);
            it.subs.contains_key(&*k)
        }
        // defined &$name / defined &{"name"}: whether the sub exists.
        Some(Expr::CallRef(f, _)) => match it.scalar(f, scope)? {
            Value::Code(_) => true,
            v => {
                let k = it.sub_key(&v.to_text());
                it.subs.contains_key(&*k)
            }
        },
        Some(Expr::Array(c)) => !it.array_of(c, scope, false).map(|a| a.borrow().is_empty()).unwrap_or(true),
        Some(Expr::Hash(c)) => !it.hash_of(c, scope, false).map(|h| h.borrow().is_empty()).unwrap_or(true),
        Some(e @ (Expr::Call { .. } | Expr::Method { .. })) => {
            let v = it.eval_call(e, scope, Want::Scalar)?;
            !v.last().map(|v| v.is_undef()).unwrap_or(true)
        }
        Some(e) => !it.scalar(e, scope)?.is_undef(),
    };
    Ok(vec![boolean(b)])
}

fn exists(it: &mut Interp, args: &[Expr], scope: &Rc<Scope>) -> ER<Vec<Value>> {
    let b = match args.first() {
        Some(Expr::HElem(c, k)) => {
            let k = it.hash_key(k, scope, "exists")?;
            let h = it.hash_of(c, scope, true)?;
            let b = h.borrow().contains(&k);
            b
        }
        Some(Expr::Elem(c, i)) => {
            let i = it.elem_index(i, scope, "exists")?;
            let a = it.array_of(c, scope, true)?;
            let len = a.borrow().len();
            index(len, i).is_some_and(|i| i < len)
        }
        Some(Expr::Call { name, .. }) => {
            let k = it.sub_key(name);
            it.subs.contains_key(&*k)
        }
        _ => return die("exists argument is not a HASH or ARRAY element or a subroutine"),
    };
    Ok(vec![boolean(b)])
}

fn delete(it: &mut Interp, args: &[Expr], scope: &Rc<Scope>) -> ER<Vec<Value>> {
    match args.first() {
        Some(Expr::HElem(c, k)) => {
            let k = it.hash_key(k, scope, "delete")?;
            let h = it.hash_of(c, scope, true)?;
            let v = h.borrow_mut().remove(&k).unwrap_or_default();
            Ok(vec![v])
        }
        Some(Expr::HSlice(c, keys)) => {
            let keys = it.list(keys, scope)?;
            let h = it.hash_of(c, scope, true)?;
            let mut h = h.borrow_mut();
            Ok(keys.iter().map(|k| h.remove(&k.to_bytes()).unwrap_or_default()).collect())
        }
        Some(Expr::Elem(c, i)) => {
            let i = it.elem_index(i, scope, "delete")?;
            let a = it.array_of(c, scope, true)?;
            let mut a = a.borrow_mut();
            let len = a.len();
            match index(len, i) {
                Some(i) if i < len => {
                    let v = std::mem::take(&mut a[i]);
                    if i + 1 == len {
                        a.pop();
                    }
                    Ok(vec![v])
                }
                _ => Ok(vec![Value::Undef]),
            }
        }
        Some(Expr::Local(inner)) => {
            let lv = it.localize(inner, scope)?;
            Ok(vec![lv.get()])
        }
        _ => die("delete argument is not a HASH or ARRAY element or slice"),
    }
}

fn substr(it: &mut Interp, args: &[Expr], scope: &Rc<Scope>) -> ER<Vec<Value>> {
    if args.len() < 2 {
        return die("Not enough arguments for substr");
    }
    if args.len() == 4 {
        let lv = it.lvalue(&args[0], scope)?;
        let off = it.scalar(&args[1], scope)?;
        it.use_num(&off, &args[1], scope, "substr", false)?;
        let off = off.to_i64();
        let len = it.scalar(&args[2], scope)?;
        it.use_num(&len, &args[2], scope, "substr", false)?;
        let len = len.to_i64();
        let repl = it.scalar(&args[3], scope)?;
        it.use_str(&repl, &args[3], scope, "substr", false)?;
        let cur = lv.get();
        let Some((old, a, b)) = substr_of(&cur, off, Some(len)) else { return die("substr outside of string") };
        lv.set(splice_str(&cur, a, b, &repl))?;
        return Ok(vec![old]);
    }
    let s = it.scalar(&args[0], scope)?;
    it.use_str(&s, &args[0], scope, "substr", false)?;
    let off = it.scalar(&args[1], scope)?;
    it.use_num(&off, &args[1], scope, "substr", false)?;
    let off = off.to_i64();
    let len = match args.get(2) {
        Some(l) => {
            let v = it.scalar(l, scope)?;
            it.use_num(&v, l, scope, "substr", false)?;
            if v.is_undef() {
                Some(0)
            } else {
                Some(v.to_i64())
            }
        }
        None => None,
    };
    Ok(vec![match substr_of(&s, off, len) {
        Some((v, _, _)) => v,
        None => {
            // a warning only where warnings are enabled: -w here (the
            // interpreter does not track `use warnings` scopes, and
            // ExifTool's modules do not enable them)
            if super::warnings_enabled(it) {
                warn(it, Value::str(&format!("substr outside of string{}", super::location_suffix())))?;
            }
            Value::Undef
        }
    }])
}

fn index_of(s: &[u8], sub: &[u8], pos: Option<i64>) -> i64 {
    let start = pos.unwrap_or(0).clamp(0, s.len() as i64) as usize;
    if sub.is_empty() {
        return start as i64;
    }
    s[start..].windows(sub.len()).position(|w| w == sub).map_or(-1, |p| (p + start) as i64)
}

fn rindex_of(s: &[u8], sub: &[u8], pos: Option<i64>) -> i64 {
    let max = pos.unwrap_or(s.len() as i64).clamp(0, s.len() as i64) as usize;
    if sub.is_empty() {
        return max as i64;
    }
    if sub.len() > s.len() {
        return -1;
    }
    let last_start = max.min(s.len() - sub.len());
    (0..=last_start).rev().find(|&i| &s[i..i + sub.len()] == sub).map_or(-1, |i| i as i64)
}

/// `hex`: an optional 0x/x prefix, underscores between digits, stops at
/// the first other character.
fn parse_radix(s: &[u8], radix: u32) -> Value {
    let mut s = s;
    if radix == 16 {
        if s.starts_with(b"0x") || s.starts_with(b"0X") {
            s = &s[2..];
        } else if s.starts_with(b"x") || s.starts_with(b"X") {
            s = &s[1..];
        }
    } else if radix == 2 {
        if s.starts_with(b"0b") || s.starts_with(b"0B") {
            s = &s[2..];
        } else if s.starts_with(b"b") || s.starts_with(b"B") {
            s = &s[1..];
        }
    }
    let mut v: u64 = 0;
    let mut f: f64 = 0.0;
    let mut overflow = false;
    for &c in s {
        if c == b'_' {
            continue;
        }
        let Some(d) = (c as char).to_digit(radix) else { break };
        if !overflow {
            match v.checked_mul(radix as u64).and_then(|x| x.checked_add(d as u64)) {
                Some(x) => v = x,
                None => {
                    overflow = true;
                    f = v as f64 * radix as f64 + d as f64;
                }
            }
        } else {
            f = f * radix as f64 + d as f64;
        }
    }
    if overflow {
        Value::Num(f)
    } else {
        crate::perl::ops::from_uv(v)
    }
}

fn oct(s: &[u8]) -> Value {
    let s = {
        let n = s.iter().take_while(|c| c.is_ascii_whitespace()).count();
        &s[n..]
    };
    if s.starts_with(b"0x") || s.starts_with(b"0X") || s.starts_with(b"x") || s.starts_with(b"X") {
        return parse_radix(s, 16);
    }
    if s.starts_with(b"0b") || s.starts_with(b"0B") || s.starts_with(b"b") || s.starts_with(b"B") {
        return parse_radix(s, 2);
    }
    let s = if s.starts_with(b"0o") || s.starts_with(b"0O") {
        &s[2..]
    } else if s.starts_with(b"o") || s.starts_with(b"O") {
        &s[1..]
    } else {
        s
    };
    parse_radix(s, 8)
}

thread_local! {
    /// perl's drand48 state (48 bits), None until seeded.
    static RAND48: RefCell<Option<u64>> = const { RefCell::new(None) };
}

/// perl's srand seeding of its drand48: x = seed << 16 | 0x330E.
fn seed_rand48(seed: u32) {
    RAND48.with(|s| *s.borrow_mut() = Some(((seed as u64) << 16) | 0x330E));
}

/// The seed srand() picks without an argument (perl's seed(): time, pid
/// and an address mixed; any value will do).
fn auto_seed() -> u32 {
    let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos() as u64).unwrap_or(0);
    let v = t ^ ((std::process::id() as u64) << 20) ^ (&t as *const u64 as u64);
    (v ^ (v >> 32)) as u32
}

/// perl's rand: drand48, x = (0x5DEECE66D * x + 0xB) mod 2**48, seeded on
/// first use if srand was not called.
fn pseudo_random() -> f64 {
    if RAND48.with(|s| s.borrow().is_none()) {
        seed_rand48(auto_seed());
    }
    RAND48.with(|s| {
        let mut st = s.borrow_mut();
        let x = st.unwrap().wrapping_mul(0x5DEECE66D).wrapping_add(0xB) & ((1u64 << 48) - 1);
        *st = Some(x);
        x as f64 / (1u64 << 48) as f64
    })
}

/// srand's argument as perl reads it (grok_number): the integer part of
/// its absolute value, or None (perl then warns and uses UV_MAX) if it is
/// not a plain decimal number within the UV range.
fn srand_seed(v: &Value) -> Option<u64> {
    if let Value::Int(i) = v {
        return Some(i.unsigned_abs());
    }
    let t = v.to_text();
    let t = t.trim_matches(|c: char| c.is_ascii_whitespace());
    let t = t.strip_prefix(['-', '+']).unwrap_or(t);
    let (int, frac) = t.split_once('.').unwrap_or((t, ""));
    if (int.is_empty() && frac.is_empty()) || !int.bytes().all(|c| c.is_ascii_digit()) || !frac.bytes().all(|c| c.is_ascii_digit()) {
        return None;
    }
    if int.is_empty() {
        return Some(0);
    }
    int.parse::<u64>().ok()
}

/// `split /PATTERN/, EXPR, LIMIT` with perl's rules (pp_split): a match
/// must end past the current position, captures are returned, trailing
/// empty fields are removed when there is no limit.
fn split(it: &mut Interp, args: &[Expr], scope: &Rc<Scope>, want: Want) -> ER<Vec<Value>> {
    let subject_v = it.arg_used(&args[1.min(args.len())..], scope, "split", false)?;
    let su = subject_v.is_utf8();
    let limit = match args.get(2) {
        Some(a) => it.scalar(a, scope)?.to_i64(),
        None => 0,
    };
    // The pattern: a match operator, ' ' (awk mode), or any expression.
    let (re, awk) = match args.first() {
        None => (crate::perl::regex::compile(b"\\s+", "").unwrap(), true),
        Some(Expr::Match { target: None, pat, .. }) => {
            let (bytes, pu) = it.pattern_form(pat, scope)?;
            if bytes == b" " {
                (it.regex_for(b"\\s+", false, "", su)?, true)
            } else if bytes == b"^" {
                (it.regex_for(b"^", false, "m", su)?, false)
            } else {
                (it.regex_for(&bytes, pu, &pat.flags, su)?, false)
            }
        }
        Some(e) => {
            let p = it.scalar(e, scope)?;
            it.use_str(&p, e, scope, "regexp compilation", false)?;
            let (p, pu) = p.str_form();
            if &p[..] == b" " {
                (it.regex_for(b"\\s+", false, "", su)?, true)
            } else {
                (it.regex_for(&p, pu, "", su)?, false)
            }
        }
    };
    let (subject, _) = subject_form(&re, &subject_v);
    let fields = crate::perl::regex::split(&re, awk, &subject, limit).map_err(|e| Flow::Die(Value::from(e)))?;
    if want != Want::List {
        return Ok(vec![Value::Int(fields.len() as i64)]);
    }
    Ok(fields)
}

#[allow(dead_code)]
fn number_of(v: &Value) -> Number {
    v.to_number()
}
