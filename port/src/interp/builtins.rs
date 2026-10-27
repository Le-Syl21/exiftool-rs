//! Perl's builtin functions, as ExifTool uses them.
//!
//! Builtins receive their argument expressions, not values: `keys %h`,
//! `push @a`, `delete $h{k}` and `defined &name` act on the storage the
//! expression names.

use std::cell::RefCell;
use std::rc::Rc;

use super::*;
use crate::perl::{pack, sprintf, Number};

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

const BUILTINS: &[&str] = &["utime", "chmod", "readpipe", "getpwuid", "getgrgid", "defined", "ref", "scalar", "lc", "uc", "lcfirst", "ucfirst", "quotemeta", "fc", "length", "chr", "ord", "hex", "oct", "abs", "int", "sqrt", "log", "exp", "sin", "cos", "atan2", "rand", "srand", "time", "localtime", "gmtime", "join", "sprintf", "reverse", "split", "pack", "unpack", "substr", "index", "rindex", "keys", "values", "each", "exists", "delete", "shift", "pop", "push", "unshift", "splice", "chomp", "chop", "undef", "pos", "die", "warn", "bless", "require", "caller", "lock", "study", "exit", "sleep", "do", "open", "close", "binmode", "unlink", "mkdir", "rmdir", "opendir", "readdir", "closedir", "system", "exec", "select", "seek", "read", "sysread", "eof", "readline", "fileno", "chdir", "rename", "stat", "lstat", "glob"];

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
            let v = arg_or_topic(it, args, scope)?;
            if v.is_undef() && name != "quotemeta" {
                return one(Value::str(""));
            }
            let b = v.to_bytes();
            let out: Vec<u8> = match name {
                "lc" | "fc" => b.to_ascii_lowercase(),
                "uc" => b.to_ascii_uppercase(),
                "lcfirst" => {
                    let mut o = b.to_vec();
                    if let Some(f) = o.first_mut() {
                        *f = f.to_ascii_lowercase();
                    }
                    o
                }
                "ucfirst" => {
                    let mut o = b.to_vec();
                    if let Some(f) = o.first_mut() {
                        *f = f.to_ascii_uppercase();
                    }
                    o
                }
                _ => crate::perl::regex::quotemeta(&b),
            };
            one(Value::bytes(out))
        }
        "length" => {
            let v = arg_or_topic(it, args, scope)?;
            if v.is_undef() {
                return one(Value::Undef);
            }
            one(Value::Int(v.to_bytes().len() as i64))
        }
        "chr" => {
            let v = arg_or_topic(it, args, scope)?;
            let n = v.to_f64();
            let n = if n < 0.0 || n.is_nan() { 0xFFFD } else { n as u32 };
            let out = if n < 256 { vec![n as u8] } else { char::from_u32(n).unwrap_or('\u{FFFD}').to_string().into_bytes() };
            one(Value::bytes(out))
        }
        "ord" => {
            let v = arg_or_topic(it, args, scope)?;
            one(Value::Int(v.to_bytes().first().copied().unwrap_or(0) as i64))
        }
        "hex" => {
            let v = arg_or_topic(it, args, scope)?;
            one(parse_radix(&v.to_bytes(), 16))
        }
        "oct" => {
            let v = arg_or_topic(it, args, scope)?;
            one(oct(&v.to_bytes()))
        }
        "abs" => arg_or_topic(it, args, scope).map(|v| vec![crate::perl::ops::abs(&v)]),
        "int" => arg_or_topic(it, args, scope).map(|v| vec![crate::perl::ops::int(&v)]),
        "sqrt" => {
            let v = arg_or_topic(it, args, scope)?.to_f64();
            if v < 0.0 {
                return die(format!("Can't take sqrt of {}", crate::perl::sprintf::format_g(v, 6, false, false)));
            }
            one(Value::Num(v.sqrt()))
        }
        "log" => {
            let v = arg_or_topic(it, args, scope)?.to_f64();
            if v <= 0.0 {
                return die(format!("Can't take log of {}", crate::perl::sprintf::format_g(v, 6, false, false)));
            }
            one(Value::Num(v.ln()))
        }
        "exp" => arg_or_topic(it, args, scope).map(|v| vec![Value::Num(v.to_f64().exp())]),
        "sin" => arg_or_topic(it, args, scope).map(|v| vec![Value::Num(v.to_f64().sin())]),
        "cos" => arg_or_topic(it, args, scope).map(|v| vec![Value::Num(v.to_f64().cos())]),
        "atan2" => {
            let a = values(it, args, scope)?;
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
        "srand" => one(Value::Int(0)),
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
            let vals = values(it, args, scope)?;
            let Some(sep) = vals.first() else { return one(Value::str("")) };
            let sep = sep.to_bytes();
            let mut out = Vec::new();
            for (k, v) in vals[1..].iter().enumerate() {
                if k > 0 {
                    out.extend_from_slice(&sep);
                }
                out.extend_from_slice(&v.to_bytes());
            }
            one(Value::bytes(out))
        }
        "sprintf" => {
            let vals = values(it, args, scope)?;
            let fmt = vals.first().map(|v| v.to_bytes()).unwrap_or_else(|| Rc::from(&b""[..]));
            one(Value::bytes(sprintf::sprintf(&fmt, &vals[1.min(vals.len())..])))
        }
        "reverse" => {
            let vals = if args.is_empty() && want != Want::List { vec![it.read_scalar("_", scope)] } else { values(it, args, scope)? };
            if want == Want::List {
                Ok(vals.into_iter().rev().collect())
            } else {
                let mut s = Vec::new();
                for v in &vals {
                    s.extend_from_slice(&v.to_bytes());
                }
                s.reverse();
                one(Value::bytes(s))
            }
        }
        "split" => split(it, args, scope, want),
        "pack" => {
            let vals = values(it, args, scope)?;
            let t = vals.first().map(|v| v.to_bytes()).unwrap_or_else(|| Rc::from(&b""[..]));
            match pack::pack(&t, &vals[1.min(vals.len())..]) {
                Ok(b) => one(Value::bytes(b)),
                Err(e) => die(e),
            }
        }
        "unpack" => {
            let t = match args.first() {
                Some(a) => it.scalar(a, scope)?.to_bytes(),
                None => return die("Not enough arguments for unpack"),
            };
            let d = match args.get(1) {
                Some(a) => it.scalar(a, scope)?.to_bytes(),
                None => it.read_scalar("_", scope).to_bytes(),
            };
            let r = pack::unpack(&t, &d);
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
            let a = values(it, args, scope)?;
            let s = a.first().map(|v| v.to_bytes()).unwrap_or_else(|| Rc::from(&b""[..]));
            let sub = a.get(1).map(|v| v.to_bytes()).unwrap_or_else(|| Rc::from(&b""[..]));
            let pos = a.get(2).map(|v| v.to_i64());
            one(Value::Int(if name == "index" { index_of(&s, &sub, pos) } else { rindex_of(&s, &sub, pos) }))
        }
        "keys" | "values" => {
            let Some(a) = args.first() else { return die(format!("Not enough arguments for {name}")) };
            let pairs: Vec<(Value, Value)> = match a {
                Expr::Hash(c) => {
                    let h = it.hash_of(c, scope, true)?;
                    let h = h.borrow();
                    h.iter().map(|(k, v)| (Value::Str(k.clone()), v.clone())).collect()
                }
                Expr::Array(c) => {
                    let ar = it.array_of(c, scope, true)?;
                    let ar = ar.borrow();
                    ar.iter().enumerate().map(|(i, v)| (Value::Int(i as i64), v.clone())).collect()
                }
                e => match it.scalar(e, scope)? {
                    Value::HashRef(h) => h.borrow().iter().map(|(k, v)| (Value::Str(k.clone()), v.clone())).collect(),
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
            for lv in flatten_lv(targets) {
                let v = lv.get();
                if v.is_undef() {
                    continue;
                }
                let s = v.to_bytes();
                if name == "chomp" {
                    if sep.is_undef() {
                        continue;
                    }
                    let sp = sep.to_bytes();
                    if sp.is_empty() {
                        let n = s.iter().rev().take_while(|&&c| c == b'\n').count();
                        if n > 0 {
                            lv.set(Value::bytes(&s[..s.len() - n]))?;
                            count += n as i64;
                        }
                    } else if s.ends_with(&sp) {
                        lv.set(Value::bytes(&s[..s.len() - sp.len()]))?;
                        count += sp.len() as i64;
                    }
                } else if !s.is_empty() {
                    last = Value::bytes(&s[s.len() - 1..]);
                    lv.set(Value::bytes(&s[..s.len() - 1]))?;
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
                Some(p) => Value::Int(p as i64),
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
                msg.extend_from_slice(b" at (eval) line 1.\n");
            }
            Err(Flow::Die(Value::bytes(msg)))
        }
        "warn" => {
            let vals = values(it, args, scope)?;
            let mut msg: Vec<u8> = vals.iter().flat_map(|v| v.to_bytes().to_vec()).collect();
            if msg.is_empty() {
                msg = b"Warning: something's wrong".to_vec();
            }
            if msg.last() != Some(&b'\n') {
                msg.extend_from_slice(b" at (eval) line 1.\n");
            }
            warn(it, Value::bytes(msg))?;
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
            if want == Want::List {
                Ok(vec![Value::str(&it.package.clone()), Value::str("(eval)"), Value::Int(1)])
            } else {
                one(Value::str(&it.package.clone()))
            }
        }
        "lock" | "study" => one(Value::Int(1)),
        "readpipe" => {
            let cmd = arg_or_topic(it, args, scope)?.to_text();
            let out = std::process::Command::new("sh").arg("-c").arg(&cmd).output();
            match out {
                Ok(o) => {
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
            let mut n = 0;
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
        Some(Value::Str(s)) if &s[..] != b"DEFAULT" && &s[..] != b"IGNORE" && !s.is_empty() => {
            let n = String::from_utf8_lossy(&s).into_owned();
            it.call_named(&n, vec![msg], Want::Void)?;
        }
        Some(Value::Str(s)) if &s[..] == b"IGNORE" => {}
        _ => {
            let h = it.handle_of(&Value::str("*STDERR"), false).unwrap();
            it.write_handle(&h, &msg.to_bytes());
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
            let k = it.hash_key(k, scope)?;
            let h = it.hash_of(c, scope, true)?;
            let b = h.borrow().contains(&k);
            b
        }
        Some(Expr::Elem(c, i)) => {
            let i = it.scalar(i, scope)?.to_i64();
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
            let k = it.hash_key(k, scope)?;
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
            let i = it.scalar(i, scope)?.to_i64();
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
        let off = it.scalar(&args[1], scope)?.to_i64();
        let len = it.scalar(&args[2], scope)?.to_i64();
        let repl = it.scalar(&args[3], scope)?;
        let s = lv.get().to_bytes();
        let Some((a, b)) = substr_range(s.len(), off, Some(len)) else { return die("substr outside of string") };
        let old = Value::bytes(&s[a..b]);
        let mut n = s[..a].to_vec();
        n.extend_from_slice(&repl.to_bytes());
        n.extend_from_slice(&s[b..]);
        lv.set(Value::bytes(n))?;
        return Ok(vec![old]);
    }
    let s = it.scalar(&args[0], scope)?.to_bytes();
    let off = it.scalar(&args[1], scope)?.to_i64();
    let len = match args.get(2) {
        Some(l) => {
            let v = it.scalar(l, scope)?;
            if v.is_undef() {
                Some(0)
            } else {
                Some(v.to_i64())
            }
        }
        None => None,
    };
    Ok(vec![match substr_range(s.len(), off, len) {
        Some((a, b)) => Value::bytes(&s[a..b]),
        None => {
            warn(it, Value::str("substr outside of string at (eval) line 1.\n"))?;
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

fn pseudo_random() -> f64 {
    thread_local!(static STATE: RefCell<u64> = RefCell::new(0x2545F4914F6CDD1D));
    STATE.with(|s| {
        let mut x = *s.borrow();
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        *s.borrow_mut() = x;
        (x >> 11) as f64 / (1u64 << 53) as f64
    })
}

/// `split /PATTERN/, EXPR, LIMIT` with perl's rules (pp_split): a match
/// must end past the current position, captures are returned, trailing
/// empty fields are removed when there is no limit.
fn split(it: &mut Interp, args: &[Expr], scope: &Rc<Scope>, want: Want) -> ER<Vec<Value>> {
    let subject = match args.get(1) {
        Some(a) => it.scalar(a, scope)?.to_bytes(),
        None => it.read_scalar("_", scope).to_bytes(),
    };
    let limit = match args.get(2) {
        Some(a) => it.scalar(a, scope)?.to_i64(),
        None => 0,
    };
    // The pattern: a match operator, ' ' (awk mode), or any expression.
    let (re, awk) = match args.first() {
        None => (crate::perl::regex::compile(b"\\s+", "").unwrap(), true),
        Some(Expr::Match { target: None, pat, .. }) => {
            let bytes = it.pattern_bytes(pat, scope)?;
            if bytes == b" " {
                (crate::perl::regex::compile(b"\\s+", "").unwrap(), true)
            } else if bytes == b"^" {
                (crate::perl::regex::compile(b"^", "m").unwrap(), false)
            } else {
                (it.regex(pat, scope)?, false)
            }
        }
        Some(e) => {
            let p = it.scalar(e, scope)?.to_bytes();
            if &p[..] == b" " {
                (crate::perl::regex::compile(b"\\s+", "").unwrap(), true)
            } else {
                match crate::perl::regex::compile(&p, "") {
                    Ok(r) => (r, false),
                    Err(e) => return die(e),
                }
            }
        }
    };
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
