//! Loading a module's data dump (`scripts/dump_module.pl`) into the
//! interpreter: package variables, with shared structures kept shared.

use std::cell::RefCell;
use std::rc::Rc;

use crate::interp::eval::{die, Binding, Code, ER};
use crate::interp::{Interp, Want};
use crate::perl::value::Hash;
use crate::perl::Value;

/// What a module does when perl loads it, besides defining its variables.
pub struct ModuleData {
    pub requires: Vec<String>,
    /// Calls made by the module's top-level code (AddCompositeTags...).
    pub calls: Vec<(String, Vec<Value>)>,
}

struct Reader<'a> {
    b: &'a [u8],
    p: usize,
    nodes: Vec<Option<Value>>,
}

impl Reader<'_> {
    fn byte(&mut self) -> Result<u8, String> {
        let c = *self.b.get(self.p).ok_or("truncated data")?;
        self.p += 1;
        Ok(c)
    }

    fn varint(&mut self) -> Result<u64, String> {
        let mut v: u64 = 0;
        let mut shift = 0;
        loop {
            let c = self.byte()?;
            v |= ((c & 0x7f) as u64) << shift;
            if c & 0x80 == 0 {
                return Ok(v);
            }
            shift += 7;
        }
    }

    fn bytes(&mut self) -> Result<&[u8], String> {
        let n = self.varint()? as usize;
        let s = self.b.get(self.p..self.p + n).ok_or("truncated data")?;
        self.p += n;
        Ok(s)
    }

    fn string(&mut self) -> Result<String, String> {
        Ok(String::from_utf8_lossy(self.bytes()?).into_owned())
    }

    fn remember(&mut self, id: u64, v: &Value) {
        let id = id as usize;
        if self.nodes.len() <= id {
            self.nodes.resize(id + 1, None);
        }
        self.nodes[id] = Some(v.clone());
    }

    /// One node. `into`: the storage a top-level variable already has.
    fn node(&mut self, it: &mut Interp, into: Option<&Binding>) -> Result<Value, String> {
        let t = self.byte()?;
        Ok(match t {
            0 => Value::Undef,
            1 => {
                let z = self.varint()?;
                Value::Int(((z >> 1) as i64) ^ -((z & 1) as i64))
            }
            11 => crate::perl::ops::from_uv(self.varint()?),
            2 => {
                let s = self.b.get(self.p..self.p + 8).ok_or("truncated data")?;
                self.p += 8;
                Value::Num(f64::from_le_bytes(s.try_into().unwrap()))
            }
            3 => Value::bytes(self.bytes()?),
            4 => {
                let id = self.varint()?;
                let n = self.varint()? as usize;
                let rc = match into {
                    Some(Binding::Array(a)) => a.clone(),
                    _ => Rc::new(RefCell::new(Vec::with_capacity(n))),
                };
                let v = Value::ArrayRef(rc.clone());
                self.remember(id, &v);
                let mut items = Vec::with_capacity(n);
                for _ in 0..n {
                    items.push(self.node(it, None)?);
                }
                *rc.borrow_mut() = items;
                v
            }
            5 => {
                let id = self.varint()?;
                let class = self.string()?;
                let n = self.varint()? as usize;
                let rc = match into {
                    Some(Binding::Hash(h)) => h.clone(),
                    _ => Rc::new(RefCell::new(Hash::default())),
                };
                if !class.is_empty() {
                    rc.borrow_mut().blessed = Some(Rc::from(class.as_str()));
                }
                let v = Value::HashRef(rc.clone());
                self.remember(id, &v);
                for _ in 0..n {
                    let k = self.bytes()?.to_vec();
                    let val = self.node(it, None)?;
                    rc.borrow_mut().insert(&k, val);
                }
                v
            }
            6 => {
                let id = self.varint()?;
                let cell = Rc::new(RefCell::new(Value::Undef));
                let v = Value::ScalarRef(cell.clone());
                self.remember(id, &v);
                *cell.borrow_mut() = self.node(it, None)?;
                v
            }
            7 => {
                let id = self.varint()? as usize;
                self.nodes.get(id).cloned().flatten().ok_or_else(|| format!("bad back reference {id}"))?
            }
            8 => {
                let name = self.string()?;
                code_by_name(it, &name)
            }
            9 => {
                let id = self.varint()?;
                let name = self.string()?;
                let src = self.bytes()?.to_vec();
                let v = anon_sub(it, &name, &src);
                self.remember(id, &v);
                v
            }
            10 => {
                let sigil = self.byte()?;
                let name = self.string()?;
                match it.global(sigil, &name) {
                    Binding::Hash(h) => Value::HashRef(h),
                    Binding::Array(a) => Value::ArrayRef(a),
                    Binding::Scalar(c) => Value::ScalarRef(c),
                }
            }
            12 => Value::bytes(self.bytes()?),
            // \*NAME: the file handle of that name
            14 => {
                let name = self.string()?;
                match it.handle_of_name(&name) {
                    Some(h) => Value::Handle(h),
                    None => Value::Undef,
                }
            }
            13 => {
                let _what = self.string()?;
                Value::Undef
            }
            t => return Err(format!("bad node type {t}")),
        })
    }
}

/// A reference to a named sub, resolved when called (the native may be
/// registered after the data that refers to it).
pub fn code_by_name(it: &mut Interp, name: &str) -> Value {
    Value::Code(it.sub_ref(name))
}

/// An anonymous sub of a table, from its deparsed source. It is compiled on
/// first call.
fn anon_sub(it: &mut Interp, name: &str, src: &[u8]) -> Value {
    let package = name.strip_suffix("::__ANON__").unwrap_or("main").to_string();
    let src = src.to_vec();
    let compiled: Rc<RefCell<Option<Rc<Code>>>> = Rc::new(RefCell::new(None));
    let _ = it;
    let n: Rc<str> = Rc::from(name);
    Value::Code(Rc::new(Code::Native(
        n,
        Rc::new(move |i: &mut Interp, args: Vec<Value>, want: Want| -> ER<Vec<Value>> {
            let existing = compiled.borrow().clone();
            let code = match existing {
                Some(c) => c,
                None => {
                    // (it sees the module's file lexicals, as the closure
                    // it was in perl: ASF's RawConv reads `my %streamType`)
                    let scope = crate::runtime::file_scope(i, &package);
                    let saved = i.strict;
                    i.strict = false;
                    let r = i.eval_string(&src, &package, &scope, Want::Scalar);
                    i.strict = saved;
                    match r {
                        Ok(mut v) => match v.pop() {
                            Some(Value::Code(c)) => {
                                *compiled.borrow_mut() = Some(c.clone());
                                c
                            }
                            _ => return die("deparsed sub did not compile to code"),
                        },
                        Err(e) => return Err(crate::interp::Flow::Die(e)),
                    }
                }
            };
            i.call(&code, args, want)
        }),
    )))
}

/// The modules a dump says its module loads.
pub fn requires(data: &[u8]) -> Result<Vec<String>, String> {
    if !data.starts_with(b"PLD1") {
        return Err("not a module dump".into());
    }
    let mut r = Reader { b: data, p: 4, nodes: Vec::new() };
    let n = r.varint()?;
    let mut out = Vec::new();
    for _ in 0..n {
        out.push(r.string()?);
    }
    Ok(out)
}

/// Load a module dump into the interpreter's package variables.
pub fn load(it: &mut Interp, module: &str, data: &[u8]) -> Result<ModuleData, String> {
    load_lexicals_as(it, module, data, None)
}

/// `load`, for a .pl file of package `pkg` (Writer.pl, WriteExif.pl...):
/// its `my` variables go under `PKG::<my>::name` unless the package's
/// module has one of that name, and always under `PKG::<my:FILE>::name`,
/// as when the file runs from its source.
pub fn load_lexicals_as(it: &mut Interp, module: &str, data: &[u8], file: Option<(&str, &str)>) -> Result<ModuleData, String> {
    if !data.starts_with(b"PLD1") {
        return Err("not a module dump".into());
    }
    let mut r = Reader { b: data, p: 4, nodes: Vec::new() };
    let n = r.varint()?;
    let mut requires = Vec::new();
    for _ in 0..n {
        requires.push(r.string()?);
    }
    // Calls are read after the variables (their arguments may refer to
    // them), so keep their bytes aside.
    let ncalls = r.varint()?;
    let mut calls_raw = Vec::new();
    for _ in 0..ncalls {
        let name = r.string()?;
        let argc = r.varint()?;
        let mut args = Vec::new();
        for _ in 0..argc {
            args.push(r.node(it, None)?);
        }
        calls_raw.push((name, args));
    }
    let nvars = r.varint()?;
    for _ in 0..nvars {
        let sigil = r.byte()?;
        let name = r.string()?;
        let existing = it.global(sigil, &name);
        match sigil {
            b'$' => {
                let v = r.node(it, None)?;
                // a variable set before the module loads (by a config file
                // or a script, as $Image::ExifTool::Geolocation::geoDir) is
                // kept: the module only declares it
                if let Binding::Scalar(c) = &existing {
                    if c.borrow().is_undef() {
                        *c.borrow_mut() = v;
                    }
                }
            }
            _ => {
                // an array or hash filled before the module loads (the
                // exiftool script pushes -config files onto @configFiles
                // first) is kept, as for a scalar: read past the dumped one
                let filled = match &existing {
                    Binding::Array(a) => !a.borrow().is_empty(),
                    Binding::Hash(h) => !h.borrow().is_empty(),
                    _ => false,
                };
                if filled {
                    r.node(it, None)?;
                    continue;
                }
                // A variable referenced earlier in the dump comes back as a
                // back reference: bind the name to that storage.
                match (r.node(it, Some(&existing))?, &existing) {
                    (Value::HashRef(h), Binding::Hash(e)) if !Rc::ptr_eq(&h, e) => it.set_global(sigil, &name, Binding::Hash(h)),
                    (Value::ArrayRef(a), Binding::Array(e)) if !Rc::ptr_eq(&a, e) => it.set_global(sigil, &name, Binding::Array(a)),
                    _ => {}
                }
            }
        }
    }
    // File lexicals: kept as package variables of an unreachable package,
    // `MODULE::<my>`, for the module's ported code.
    let nlex = r.varint()?;
    for _ in 0..nlex {
        let sigil = r.byte()?;
        let var = r.string()?;
        let name = match file {
            Some((pkg, f)) => format!("{pkg}::<my:{f}>::{var}"),
            None => format!("{}::<my>::{}", module, var),
        };
        let existing = it.global(sigil, &name);
        match sigil {
            b'$' => {
                let v = r.node(it, None)?;
                if let Binding::Scalar(c) = &existing {
                    *c.borrow_mut() = v;
                }
            }
            _ => match (r.node(it, Some(&existing))?, &existing) {
                (Value::HashRef(h), Binding::Hash(e)) if !Rc::ptr_eq(&h, e) => it.set_global(sigil, &name, Binding::Hash(h)),
                (Value::ArrayRef(a), Binding::Array(e)) if !Rc::ptr_eq(&a, e) => it.set_global(sigil, &name, Binding::Array(a)),
                _ => {}
            },
        }
    }
    // a .pl file's lexicals are also the package's own unless it has them
    if let Some((pkg, f)) = file {
        let own = format!("{pkg}::<my:{f}>::");
        for (sigil, name, b) in it.globals_with_prefix(&own) {
            let shared = format!("{pkg}::<my>::{}", &name[own.len()..]);
            if it.globals_with_prefix(&shared).iter().all(|(s, n, _)| *s != sigil || n != &shared) {
                it.set_global(sigil, &shared, b);
            }
        }
    }
    Ok(ModuleData { requires, calls: calls_raw })
}
