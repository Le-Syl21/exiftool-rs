//! Accessors on Perl values for ported code: `$$h{KEY}`, `$$a[i]`, and the
//! like, without the ceremony of the reference types.
//!
//! The ported subs keep ExifTool's data structures (the ExifTool object, tag
//! tables, tagInfo hashes) as Perl hashes, as the table strings read them
//! directly; these helpers make that code read like the Perl it comes from.

use std::cell::RefCell;
use std::rc::Rc;

use super::value::{Hash, Value};

impl Value {
    /// A new empty hash reference: `{}`.
    pub fn new_hash() -> Value {
        Value::HashRef(Rc::new(RefCell::new(Hash::default())))
    }

    /// A new array reference: `[ ... ]`.
    pub fn new_array(v: Vec<Value>) -> Value {
        Value::ArrayRef(Rc::new(RefCell::new(v)))
    }

    pub fn is_hash(&self) -> bool {
        matches!(self, Value::HashRef(_))
    }

    pub fn is_array(&self) -> bool {
        matches!(self, Value::ArrayRef(_))
    }

    pub fn is_scalar_ref(&self) -> bool {
        matches!(self, Value::ScalarRef(_) | Value::LvRef(_))
    }

    pub fn is_code(&self) -> bool {
        matches!(self, Value::Code(_))
    }

    /// `ref $v`.
    pub fn ref_type(&self) -> &str {
        match self {
            Value::ScalarRef(c) => {
                if c.borrow().is_ref() {
                    "REF"
                } else {
                    "SCALAR"
                }
            }
            Value::ArrayRef(_) => "ARRAY",
            Value::HashRef(_) => "HASH",
            Value::Code(_) => "CODE",
            Value::Handle(_) => "GLOB",
            Value::LvRef(l) => {
                if l.get().is_ref() {
                    "REF"
                } else {
                    "SCALAR"
                }
            }
            _ => "",
        }
    }

    /// `$$v{key}` (undef when `$v` is not a hash reference or the key is
    /// missing; no autovivification).
    pub fn get(&self, key: impl AsRef<[u8]>) -> Value {
        match self {
            Value::HashRef(h) => h.borrow().get(key.as_ref()).cloned().unwrap_or_default(),
            _ => Value::Undef,
        }
    }

    /// `$$v{a}{b}...`
    pub fn path(&self, keys: &[&str]) -> Value {
        let mut v = self.clone();
        for k in keys {
            v = v.get(k);
        }
        v
    }

    /// `$$v{key} = $val` on a hash reference.
    pub fn set(&self, key: impl AsRef<[u8]>, val: impl Into<Value>) {
        if let Value::HashRef(h) = self {
            h.borrow_mut().insert(key.as_ref(), val.into());
        }
    }

    /// `exists $$v{key}`.
    pub fn exists(&self, key: impl AsRef<[u8]>) -> bool {
        match self {
            Value::HashRef(h) => h.borrow().contains(key.as_ref()),
            _ => false,
        }
    }

    /// `delete $$v{key}`.
    pub fn delete(&self, key: impl AsRef<[u8]>) -> Value {
        match self {
            Value::HashRef(h) => h.borrow_mut().remove(key.as_ref()).unwrap_or_default(),
            _ => Value::Undef,
        }
    }

    /// `keys %$v`, in the hash's order.
    pub fn keys(&self) -> Vec<Rc<[u8]>> {
        match self {
            Value::HashRef(h) => h.borrow().keys().cloned().collect(),
            _ => Vec::new(),
        }
    }

    /// `$$v{key}`, creating an empty hash there when it is undefined
    /// (perl's autovivification of `$$v{key}{...}`).
    pub fn get_or_new_hash(&self, key: impl AsRef<[u8]>) -> Value {
        let cur = self.get(key.as_ref());
        if cur.is_undef() {
            let h = Value::new_hash();
            self.set(key, h.clone());
            return h;
        }
        cur
    }

    pub fn get_or_new_array(&self, key: impl AsRef<[u8]>) -> Value {
        let cur = self.get(key.as_ref());
        if cur.is_undef() {
            let a = Value::new_array(Vec::new());
            self.set(key, a.clone());
            return a;
        }
        cur
    }

    /// `$$v[i]` (negative from the end).
    pub fn at(&self, i: i64) -> Value {
        match self {
            Value::ArrayRef(a) => {
                let a = a.borrow();
                let idx = if i < 0 { a.len() as i64 + i } else { i };
                if idx < 0 {
                    return Value::Undef;
                }
                a.get(idx as usize).cloned().unwrap_or_default()
            }
            _ => Value::Undef,
        }
    }

    /// `$$v[i] = $val`.
    pub fn set_at(&self, i: usize, val: impl Into<Value>) {
        if let Value::ArrayRef(a) = self {
            let mut a = a.borrow_mut();
            if i >= a.len() {
                a.resize(i + 1, Value::Undef);
            }
            a[i] = val.into();
        }
    }

    /// `push @$v, $val`.
    pub fn push(&self, val: impl Into<Value>) {
        if let Value::ArrayRef(a) = self {
            a.borrow_mut().push(val.into());
        }
    }

    /// `unshift @$v, $val`.
    pub fn unshift(&self, val: impl Into<Value>) {
        if let Value::ArrayRef(a) = self {
            a.borrow_mut().insert(0, val.into());
        }
    }

    /// `@$v` (a copy of the elements).
    pub fn items(&self) -> Vec<Value> {
        match self {
            Value::ArrayRef(a) => a.borrow().clone(),
            _ => Vec::new(),
        }
    }

    /// `scalar @$v` / `scalar keys %$v`.
    pub fn len(&self) -> usize {
        match self {
            Value::ArrayRef(a) => a.borrow().len(),
            Value::HashRef(h) => h.borrow().len(),
            _ => 0,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// `$$v` of a scalar reference.
    pub fn deref(&self) -> Value {
        match self {
            Value::ScalarRef(c) => c.borrow().clone(),
            Value::LvRef(l) => l.get(),
            _ => Value::Undef,
        }
    }

    /// `ref $a eq ref $b and $a == $b`: the same referenced storage.
    pub fn same_ref(&self, other: &Value) -> bool {
        match (self, other) {
            (Value::HashRef(a), Value::HashRef(b)) => Rc::ptr_eq(a, b),
            (Value::ArrayRef(a), Value::ArrayRef(b)) => Rc::ptr_eq(a, b),
            (Value::ScalarRef(a), Value::ScalarRef(b)) => Rc::ptr_eq(a, b),
            (Value::Code(a), Value::Code(b)) => Rc::ptr_eq(a, b),
            (Value::LvRef(a), Value::LvRef(b)) => Rc::ptr_eq(a, b),
            _ => false,
        }
    }

    /// The string form compared with `eq`.
    pub fn eq_str(&self, s: &str) -> bool {
        !self.is_undef() && &self.to_bytes()[..] == s.as_bytes()
    }

    /// A shallow copy of a hash: `{ %$v }`.
    pub fn hash_copy(&self) -> Value {
        match self {
            Value::HashRef(h) => {
                let mut c = h.borrow().clone();
                c.blessed = None;
                Value::HashRef(Rc::new(RefCell::new(c)))
            }
            _ => Value::new_hash(),
        }
    }
}

impl From<bool> for Value {
    fn from(b: bool) -> Value {
        if b {
            Value::Int(1)
        } else {
            Value::str("")
        }
    }
}

impl From<usize> for Value {
    fn from(i: usize) -> Value {
        Value::Int(i as i64)
    }
}

impl From<i32> for Value {
    fn from(i: i32) -> Value {
        Value::Int(i as i64)
    }
}

impl From<Vec<u8>> for Value {
    fn from(b: Vec<u8>) -> Value {
        Value::bytes(b)
    }
}

impl From<&[u8]> for Value {
    fn from(b: &[u8]) -> Value {
        Value::bytes(b)
    }
}

impl From<Rc<[u8]>> for Value {
    fn from(b: Rc<[u8]>) -> Value {
        Value::Str(b)
    }
}
