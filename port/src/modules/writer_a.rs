//! Image::ExifTool::Writer (Image/ExifTool/Writer.pl), half A of its subs, ported sub by sub (see docs/port/PORTING.md).
//! This half: the subs from `SetNewValue` to `GetExtended` (lines 312-4520).

use std::cell::RefCell;
use std::rc::Rc;

use crate::interp::eval::{die, Cell, ER};
use crate::interp::{Binding, Code, Flow, Interp, Scope, Want};
use crate::perl::{ops, Value};
use crate::pl::{self, arg, cap, re, ret};

const ET: &str = "Image::ExifTool";

/// `$evalWarning` (a `use vars` variable of ExifTool.pm)
const EVAL_WARNING: &str = "Image::ExifTool::evalWarning";

/// `$advFmtSelf` (a `use vars` variable of ExifTool.pm)
const ADV_FMT_SELF: &str = "Image::ExifTool::advFmtSelf";

/// The package of Writer.pl's file lexicals (`my %tiffMap`...), as the
/// loader keeps them for a required .pl file.
const MY: &str = "Image::ExifTool::<my:Writer.pl>::";

// ---- local helpers (Perl operations the shared helpers do not cover) ----

fn n(i: i64) -> Value {
    Value::Int(i)
}

fn s(x: &str) -> Value {
    Value::str(x)
}

/// A file lexical hash of Writer.pl (`my %name`).
fn my_hash(it: &mut Interp, name: &str) -> Value {
    match it.global(b'%', &format!("{MY}{name}")) {
        Binding::Hash(h) => Value::HashRef(h),
        _ => unreachable!(),
    }
}

/// A file lexical array of Writer.pl (`my @name`).
fn my_array(it: &mut Interp, name: &str) -> Value {
    match it.global(b'@', &format!("{MY}{name}")) {
        Binding::Array(a) => Value::ArrayRef(a),
        _ => unreachable!(),
    }
}

/// A file lexical scalar of Writer.pl (`my $name`).
fn my_scalar(it: &mut Interp, name: &str) -> Cell {
    match it.global(b'$', &format!("{MY}{name}")) {
        Binding::Scalar(c) => c,
        _ => unreachable!(),
    }
}

/// A scope holding Writer.pl's file lexicals, as the outer scope of its
/// eval sites (the code sees them as perl's eval does).
fn writer_scope(it: &Interp) -> Rc<Scope> {
    let scope = Scope::new(None);
    for (sigil, name, b) in it.globals_with_prefix(MY) {
        scope.declare(sigil, &name[MY.len()..], b);
    }
    scope
}


/// `keys %$h` as values.
fn keys(h: &Value) -> Vec<Value> {
    h.keys().into_iter().map(Value::Str).collect()
}

/// `values %$h`.
fn values(h: &Value) -> Vec<Value> {
    match h {
        Value::HashRef(r) => r.borrow().values().cloned().collect(),
        _ => Vec::new(),
    }
}

/// `$a cmp $b`
fn cmp(a: &Value, b: &Value) -> std::cmp::Ordering {
    ops::str_cmp(a, b)
}

/// `$a <=> $b` (undef, as for NaN, counts as 0 in a sort block).
fn ncmp(a: &Value, b: &Value) -> std::cmp::Ordering {
    ops::num_cmp(a, b).unwrap_or(std::cmp::Ordering::Equal)
}

/// `sort LIST`
fn sort_str(mut v: Vec<Value>) -> Vec<Value> {
    v.sort_by(cmp);
    v
}

/// `lc $v` (undef is "")
fn lc(v: &Value) -> Value {
    pl::lc(v)
}

/// `$a eq $b`
fn eq(a: &Value, b: &Value) -> bool {
    a.to_bytes()[..] == b.to_bytes()[..]
}

/// `defined $v and length $v` is `length $v` truthy.
fn length_of(v: &Value) -> i64 {
    v.to_bytes().len() as i64
}

/// `$$h{KEY}` as the base of a longer element access (`$$h{KEY}{...}`):
/// perl creates the inner hash when it is undefined, even to read it.
fn viv(h: &Value, key: impl AsRef<[u8]>) -> Value {
    if h.is_hash() {
        h.get_or_new_hash(key)
    } else {
        Value::Undef
    }
}

/// `$$h{KEY}` as the base of an array access (`push @{$$h{KEY}}, ...`).
fn viv_array(h: &Value, key: impl AsRef<[u8]>) -> Value {
    if h.is_hash() {
        h.get_or_new_array(key)
    } else {
        Value::Undef
    }
}

/// `@$ref = LIST`: replace the contents of an existing array in place.
fn array_assign(r: &Value, items: Vec<Value>) {
    if let Value::ArrayRef(a) = r {
        *a.borrow_mut() = items;
    }
}

/// `bless {...}` into a class.
fn bless(h: &Value, class: &str) {
    if let Value::HashRef(r) = h {
        r.borrow_mut().blessed = Some(Rc::from(class));
    }
}


/// `$obj->Method(ARGS)` in void context.
fn method_void(it: &mut Interp, obj: &Value, name: &str, args: Vec<Value>) -> ER<()> {
    let mut a = vec![obj.clone()];
    a.extend(args);
    it.call_method(obj, name, a, Want::Void)?;
    Ok(())
}

/// `Func(ARGS)` in void context.
fn call_void(it: &mut Interp, name: &str, args: Vec<Value>) -> ER<()> {
    it.call_named(name, args, Want::Void)?;
    Ok(())
}

/// `\&SetWarning`
fn set_warning_ref(it: &mut Interp) -> Value {
    Value::Code(it.sub_ref("Image::ExifTool::SetWarning"))
}

/// `CleanWarning()`
fn clean_warning(it: &mut Interp) -> ER<Value> {
    pl::call(it, "Image::ExifTool::CleanWarning", vec![])
}

/// `$^O`
fn os(it: &mut Interp) -> Value {
    it.get_scalar("main::^O")
}

/// `print $out LIST`
fn print_out(it: &mut Interp, out: &Value, msg: Value) -> ER<()> {
    pl::print_to(it, Some(out), &[msg])?;
    Ok(())
}


/// Concatenate several values (`"$a$b$c"`).
fn cat(parts: &[&Value]) -> Value {
    let mut v = Vec::new();
    for p in parts {
        v.extend_from_slice(&p.to_bytes());
    }
    Value::bytes(v)
}

/// `return LIST`: the list, or its last element in scalar context (a list
/// literal), as perl.
fn ret_list(v: Vec<Value>, want: Want) -> ER<Vec<Value>> {
    if want == Want::List {
        Ok(v)
    } else {
        Ok(vec![v.last().cloned().unwrap_or_default()])
    }
}

/// `return @array` (or `grep`): the list, or its count in scalar context.
fn ret_count(v: Vec<Value>, want: Want) -> ER<Vec<Value>> {
    if want == Want::List {
        Ok(v)
    } else {
        ret(n(v.len() as i64))
    }
}

/// `return sort ...`: the list, or undef in scalar context (as perl).
fn ret_sorted(v: Vec<Value>, want: Want) -> ER<Vec<Value>> {
    if want == Want::List {
        Ok(v)
    } else {
        pl::ret_undef()
    }
}

/// `split /RE/, $s` with a pattern given as text.
fn split_on(pat: &str, v: &Value) -> Vec<Value> {
    pl::split(&re(pat, ""), v, 0)
}

/// `grep /^$pat$/i, LIST` for an interpolated pattern.
fn grep_dyn(pat: &[u8], flags: &str, list: &[Value]) -> ER<Vec<Value>> {
    let r = pl::re_dyn(pat, flags)?;
    Ok(list.iter().filter(|v| r.is_match(v)).cloned().collect())
}

/// `/^PAT$/FLAGS` with PAT interpolated (no quotemeta).
fn anchored(p: &Value) -> Vec<u8> {
    let mut v = b"^".to_vec();
    v.extend_from_slice(&p.to_bytes());
    v.push(b'$');
    v
}

/// Run a snippet of Perl (a primitive no helper provides: `stat`, `chmod`,
/// `\*FH`, `close FH`, `link`...) with the scalars `vars` bound as
/// lexicals; returns its result and the variables' values afterwards. `$@`
/// is left as it was; a die or exit goes through.
fn perl_snippet(it: &mut Interp, code: &str, vars: &[(&str, Value)], want: Want) -> ER<(Vec<Value>, Vec<Value>)> {
    let scope = Scope::new(None);
    let cells: Vec<_> = vars.iter().map(|(name, v)| scope.scalar(name, v.clone())).collect();
    let saved_err = it.get_scalar("main::@");
    let had_exit = it.exit_code.is_some();
    let r = it.eval_string(code.as_bytes(), ET, &scope, want);
    it.set_scalar("main::@", saved_err);
    let out: Vec<Value> = cells.iter().map(|c| c.borrow().clone()).collect();
    match r {
        Ok(v) => Ok((v, out)),
        Err(e) => {
            if !had_exit {
                if let Some(code) = it.exit_code.take() {
                    return Err(Flow::Exit(code));
                }
            }
            Err(Flow::Die(e))
        }
    }
}

/// A snippet's scalar result.
fn snippet1(it: &mut Interp, code: &str, vars: &[(&str, Value)]) -> ER<Value> {
    Ok(perl_snippet(it, code, vars, Want::Scalar)?.0.pop().unwrap_or_default())
}

/// `\*NAME` (a glob reference, in package Image::ExifTool).
fn glob_ref(it: &mut Interp, name: &str) -> ER<Value> {
    snippet1(it, &format!("\\*{name}"), &[])
}

/// Run an eval-site string in `scope` (`eval $code`): the result, or undef
/// with `$@` set when it died. An exit inside goes through.
fn eval_in(it: &mut Interp, code: &Value, scope: &Rc<Scope>, want: Want) -> ER<Value> {
    let had_exit = it.exit_code.is_some();
    match it.eval_string(&code.to_bytes(), ET, scope, want) {
        Ok(mut v) => Ok(v.pop().unwrap_or_default()),
        Err(_) => {
            if !had_exit {
                if let Some(c) = it.exit_code.take() {
                    return Err(Flow::Exit(c));
                }
            }
            Ok(Value::Undef)
        }
    }
}

/// `$@`
fn eval_error(it: &mut Interp) -> Value {
    it.get_scalar("main::@")
}

/// `hex($v)` for a string of hex digits.
fn hex(v: &Value) -> Value {
    let b = v.to_bytes();
    let txt: String = b.iter().filter(|c| c.is_ascii_hexdigit()).map(|&c| c as char).collect();
    match u64::from_str_radix(if txt.is_empty() { "0" } else { &txt }, 16) {
        Ok(u) => ops::from_uv(u),
        Err(_) => {
            let mut f = 0f64;
            for c in txt.chars() {
                f = f * 16.0 + c.to_digit(16).unwrap() as f64;
            }
            Value::Num(f)
        }
    }
}

/// A Perl sub made of a Rust closure (for `sub { ... }` values).
fn closure(name: &str, f: impl Fn(&mut Interp, Vec<Value>, Want) -> ER<Vec<Value>> + 'static) -> Value {
    Value::Code(Rc::new(Code::Native(Rc::from(name), Rc::new(f))))
}

/// `$v++` on a hash element (returns the old value, 0 for undef).
fn post_inc(h: &Value, key: &str) -> Value {
    let old = h.get(key);
    h.set(key, pl::add(&old, &n(1)));
    if old.is_undef() {
        n(0)
    } else {
        old
    }
}

/// `++$$h{key}`
fn pre_inc(h: &Value, key: &str) -> Value {
    let v = pl::add(&h.get(key), &n(1));
    h.set(key, v.clone());
    v
}

/// `%{$v}` as an rvalue: dies as perl does when `$v` is not a hash ref.
fn hash_rv(v: &Value) -> ER<Value> {
    match v {
        Value::HashRef(_) => Ok(v.clone()),
        Value::Undef => die("Can't use an undefined value as a HASH reference"),
        _ => die(format!("Can't use string (\"{}\") as a HASH ref while \"strict refs\" in use", v.to_text())),
    }
}

/// `$$self{OPTIONS}{name}`
fn opt(self_: &Value, name: &str) -> Value {
    viv(self_, "OPTIONS").get(name)
}

/// `GetByteOrder() eq 'MM'`
fn is_mm(it: &mut Interp) -> ER<bool> {
    Ok(pl::str_eq(&pl::call(it, "Image::ExifTool::GetByteOrder", vec![])?, "MM"))
}

// ---- the subs ----

/// Uncommon utility routines to for reading binary data values
/// Inputs: 0) data reference, 1) offset into data
pub fn get64s(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (data_pt, pos) = (arg(&a, 0), arg(&a, 1));
    let pt = if is_mm(it)? { n(0) } else { n(4) }; // get position of high word
    let hi = pl::call(it, "Image::ExifTool::Get32s", vec![data_pt.clone(), pl::add(&pos, &pt)])?; // preserve sign bit of high word
    let lo = pl::call(it, "Image::ExifTool::Get32u", vec![data_pt, pl::sub(&pl::add(&pos, &n(4)), &pt)])?;
    ret(pl::add(&pl::mul(&hi, &n(4294967296)), &lo))
}

pub fn get64u(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (data_pt, pos) = (arg(&a, 0), arg(&a, 1));
    let pt = if is_mm(it)? { n(0) } else { n(4) }; // get position of high word
    let hi = pl::call(it, "Image::ExifTool::Get32u", vec![data_pt.clone(), pl::add(&pos, &pt)])?; // (unsigned this time)
    let lo = pl::call(it, "Image::ExifTool::Get32u", vec![data_pt, pl::sub(&pl::add(&pos, &n(4)), &pt)])?;
    ret(pl::add(&pl::mul(&hi, &n(4294967296)), &lo))
}

pub fn get_fixed64s(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (data_pt, pos) = (arg(&a, 0), arg(&a, 1));
    let v = get64s(it, vec![data_pt, pos], Want::Scalar)?.pop().unwrap_or_default();
    let val = pl::div(&v, &n(4294967296))?;
    // remove insignificant digits
    let half = if pl::gt(&val, &n(0)) { Value::Num(0.5) } else { Value::Num(-0.5) };
    ret(pl::div(&pl::int(&pl::add(&pl::mul(&val, &Value::Num(1e10)), &half)), &Value::Num(1e10))?)
}

/// Decode extended 80-bit float used by Apple SANE and Intel 8087
/// (note: different than the IEEE standard 80-bit float)
pub fn get_extended(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (data_pt, pos) = (arg(&a, 0), arg(&a, 1));
    let pt = if is_mm(it)? { n(0) } else { n(2) }; // get position of exponent
    let mut exp = pl::call(it, "Image::ExifTool::Get16u", vec![data_pt.clone(), pl::add(&pos, &pt)])?;
    let sig = get64u(it, vec![data_pt, pl::sub(&pl::add(&pos, &n(2)), &pt)], Want::Scalar)?.pop().unwrap_or_default(); // get significand as int64u
    let sign = if ops::bit_and(&exp, &n(0x8000)).truthy() { n(-1) } else { n(1) };
    exp = pl::sub(&pl::sub(&ops::bit_and(&exp, &n(0x7fff)), &n(16383)), &n(63)); // (-63 to fractionalize significand)
    ret(pl::mul(&pl::mul(&sign, &sig), &ops::pow(&n(2), &exp)))
}

/// Is this a raw file type?
/// Inputs: 0) ExifTool ref
/// Returns: true if FileType is a type of RAW image
pub fn is_raw_type(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let self_ = arg(&a, 0);
    let raw_type = my_hash(it, "rawType");
    ret(raw_type.get(self_.get("FileType").to_bytes()))
}

/// Get new file path name
/// Inputs: 0) existing name (may contain directory),
///         1) new file name, new directory, or new path (dir+name)
/// Returns: new file path name
pub fn get_new_file_name(_it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (old_name, mut new_name) = (arg(&a, 0), arg(&a, 1));
    let (mut dir, mut name) = match re(r"(.*/)(.*)", "").m(&old_name) {
        Some(c) => (cap(&c, 1), cap(&c, 2)),
        None => (Value::Undef, Value::Undef),
    };
    if dir.is_undef() {
        dir = s("");
        name = old_name.clone();
    }
    if re(r"/$", "").is_match(&new_name) {
        new_name = pl::concat(&new_name, &name); // change dir only
    } else if !re(r"/", "").is_match(&new_name) {
        new_name = pl::concat(&dir, &new_name); // change name only if newname doesn't specify dir
    } // else change dir and name
    ret(new_name)
}

fn new_file_name(it: &mut Interp, old: &Value, new: &Value) -> ER<Value> {
    Ok(get_new_file_name(it, vec![old.clone(), new.clone()], Want::Scalar)?.pop().unwrap_or_default())
}

/// Get next available tag key
/// Inputs: 0) hash reference (keys are tag keys), 1) tag name
/// Returns: next available tag key
pub fn next_free_tag_key(_it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (info, tag) = (arg(&a, 0), arg(&a, 1));
    if !info.exists(tag.to_bytes()) {
        return ret(tag);
    }
    let mut i = 1;
    loop {
        let key = pl::concat(&tag, &s(&format!(" ({i})")));
        if !info.exists(key.to_bytes()) {
            return ret(key);
        }
        i += 1;
    }
}

fn next_free(info: &Value, tag: &str) -> Value {
    let t = s(tag);
    if !info.exists(t.to_bytes()) {
        return t;
    }
    let mut i = 1;
    loop {
        let key = s(&format!("{tag} ({i})"));
        if !info.exists(key.to_bytes()) {
            return key;
        }
        i += 1;
    }
}

/// Get write group for specified tag
/// Inputs: 0) new value hash reference
/// Returns: Write group name
pub fn get_write_group(_it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    ret(arg(&a, 0).get("WriteGroup"))
}

/// Get name of write group or family 1 group
/// Inputs: 0) ExifTool ref, 1) tagInfo ref, 2) write group name
/// Returns: Name of group for verbose message
pub fn get_write_group1(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (self_, tag_info, write_group) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    if !re(r"^(MakerNotes|XMP|Composite|QuickTime)$", "").is_match(&write_group) {
        return ret(write_group);
    }
    ret(pl::method(it, &self_, "GetGroup", vec![tag_info, n(1)])?)
}

fn write_group1(it: &mut Interp, self_: &Value, tag_info: &Value, write_group: &Value) -> ER<Value> {
    Ok(get_write_group1(it, vec![self_.clone(), tag_info.clone(), write_group.clone()], Want::Scalar)?.pop().unwrap_or_default())
}

/// Return the total number of new values set
/// Inputs: 0) ExifTool object reference
/// Returns: Scalar context) Number of new values that have been set (incl pseudo)
///          List context) Number of new values (incl pseudo), number of "pseudo" values
/// ("pseudo" values are those which don't require rewriting the file to change)
pub fn count_new_values(_it: &mut Interp, a: Vec<Value>, want: Want) -> ER<Vec<Value>> {
    let self_ = arg(&a, 0);
    let new_val = self_.get("NEW_VALUE");
    let (mut num, mut pseudo) = (n(0), n(0));
    if new_val.truthy() {
        num = n(new_val.len() as i64);
        for nv in values(&new_val) {
            let tag_info = nv.get("TagInfo");
            // don't count tags that don't write anything
            if tag_info.get("WriteNothing").truthy() {
                num = pl::sub(&num, &n(1));
                continue;
            }
            // count the number of pseudo tags included
            if tag_info.get("WritePseudo").truthy() {
                pseudo = pl::add(&pseudo, &n(1));
            }
        }
    }
    num = pl::add(&num, &n(viv(&self_, "DEL_GROUP").len() as i64));
    if want != Want::List {
        return ret(num);
    }
    Ok(vec![num, pseudo])
}

/// Save new values for subsequent restore
/// Inputs: 0) ExifTool object reference
/// Returns: Number of times new values have been saved
/// Notes: increments SAVE_COUNT flag each time routine is called
pub fn save_new_values(_it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let self_ = arg(&a, 0);
    let new_values = self_.get("NEW_VALUE");
    let save_count = pre_inc(&self_, "SAVE_COUNT");
    for key in keys(&new_values) {
        let mut nv_hash = new_values.get(key.to_bytes());
        while nv_hash.truthy() {
            // set Save count if not done already
            if !nv_hash.get("Save").truthy() {
                nv_hash.set("Save", save_count.clone());
            }
            nv_hash = nv_hash.get("Next");
        }
    }
    // initialize hash for saving overwritten new values
    self_.set("SAVE_NEW_VALUE", Value::new_hash());
    // make a copy of the delete group hash
    let del_grp = hash_rv(&self_.get("DEL_GROUP"))?.hash_copy();
    self_.set("SAVE_DEL_GROUP", del_grp);
    ret(save_count)
}

/// Restore new values to last saved state
/// Inputs: 0) ExifTool object reference
/// Notes: Restores saved new values, but currently doesn't restore them in the
/// original order, so there may be some minor side-effects when restoring tags
/// with overlapping groups. eg) XMP:Identifier, XMP-dc:Identifier
/// Also, this doesn't do the right thing for list-type tags which accumulate
/// values across a save point
pub fn restore_new_values(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let self_ = arg(&a, 0);
    let mut new_values = self_.get("NEW_VALUE");
    let saved_values = self_.get("SAVE_NEW_VALUE");
    // 1) remove any new values which don't have the Save flag set
    if new_values.truthy() {
        let ks = keys(&new_values);
        for key in ks {
            let key = key.to_bytes();
            let mut last_hash = Value::Undef;
            let mut nv_hash = new_values.get(&key);
            while nv_hash.truthy() {
                if nv_hash.get("Save").truthy() {
                    last_hash = nv_hash.clone();
                } else {
                    // remove this entry from the list
                    if last_hash.truthy() {
                        last_hash.set("Next", nv_hash.get("Next"));
                    } else if nv_hash.get("Next").truthy() {
                        new_values.set(&key, nv_hash.get("Next"));
                    } else {
                        new_values.delete(&key);
                    }
                }
                nv_hash = nv_hash.get("Next");
            }
        }
    }
    // 2) restore saved new values
    if saved_values.truthy() {
        if !new_values.truthy() {
            new_values = Value::new_hash();
            self_.set("NEW_VALUE", new_values.clone());
        }
        for key in keys(&saved_values) {
            let key = key.to_bytes();
            if new_values.get(&key).truthy() {
                // add saved values to end of list
                let nv_hash = pl::call(it, "Image::ExifTool::LastInList", vec![new_values.get(&key)])?;
                nv_hash.set("Next", saved_values.get(&key));
            } else {
                new_values.set(&key, saved_values.get(&key));
            }
        }
        self_.set("SAVE_NEW_VALUE", Value::new_hash()); // reset saved new values
    }
    // 3) restore delete groups
    let del_grp = hash_rv(&self_.get("SAVE_DEL_GROUP"))?.hash_copy();
    self_.set("DEL_GROUP", del_grp.clone());
    ret(del_grp)
}

/// Set alternate file for extracting information
/// Inputs: 0) ExifTool ref, 1) family 8 group name (of the form "File#" where # is any number)
///         2) alternate file name (may contain tag names with leading "$"), or undef to reset
/// Returns: 1 on success, or 0 on invalid group name
pub fn set_alternate_file(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (self_, g8, file) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    let g8 = pl::ucfirst(&lc(&g8));
    if !re(r"^File\d+$", "").is_match(&g8) {
        return ret(n(0));
    }
    let g8k = g8.to_bytes();
    // keep the same file if already initialized (possibly has metadata extracted)
    if file.is_undef() {
        viv(&self_, "ALT_EXIFTOOL").delete(&g8k);
    } else {
        let alt = viv(&self_, "ALT_EXIFTOOL");
        let same = alt.get(&g8k).truthy() && !re(r"\$", "").is_match(&file) && eq(&alt.get(&g8k).get("ALT_FILE"), &file);
        if !same {
            let alt_exif_tool = pl::method(it, &s(ET), "new", vec![])?;
            alt_exif_tool.set("ALT_FILE", file);
            viv(&self_, "ALT_EXIFTOOL").set(&g8k, alt_exif_tool);
        }
    }
    ret(n(1))
}

/// get priority group list for new values
/// Inputs: 0) ExifTool object reference
/// Returns: List of group names
pub fn get_new_groups(_it: &mut Interp, a: Vec<Value>, want: Want) -> ER<Vec<Value>> {
    let self_ = arg(&a, 0);
    ret_count(self_.get("WRITE_GROUPS").items(), want)
}

/// Get list of all deletable group names
/// Returns: List of group names (sorted alphabetically)
pub fn get_delete_groups(it: &mut Interp, _a: Vec<Value>, want: Want) -> ER<Vec<Value>> {
    let mut list = my_array(it, "delGroups").items();
    list.extend(my_array(it, "delGroup2").items());
    list.sort_by(|x, y| cmp(&lc(x), &lc(y)));
    ret_sorted(list, want)
}

/// Add user-defined tags at run time
/// Inputs: 0) destination table name, 1) tagID/tagInfo pairs for tags to add
/// Returns: number of tags added
/// Notes: will replace existing tags
pub fn add_user_defined_tags(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    pl::local_topic(it, |it| {
        let table_name = arg(&a, 0);
        let add_tags = Value::new_hash();
        let mut i = 1;
        while i < a.len() {
            add_tags.set(a[i].to_bytes(), arg(&a, i + 1));
            i += 2;
        }
        let table = pl::call(it, "Image::ExifTool::GetTagTable", vec![table_name.clone()])?;
        if !table.truthy() {
            return ret(n(0));
        }
        // add tags to writer lookup
        call_void(it, "Image::ExifTool::TagLookup::AddTags", vec![add_tags.clone(), table_name])?;
        let mut num = 0;
        let special_tags = pl::pkg_hash(it, "Image::ExifTool::specialTags");
        for tag_id in keys(&add_tags) {
            if special_tags.get(tag_id.to_bytes()).truthy() {
                continue;
            }
            table.delete(tag_id.to_bytes()); // delete old entry if it existed
            call_void(it, "Image::ExifTool::AddTagToTable", vec![table.clone(), tag_id.clone(), add_tags.get(tag_id.to_bytes()), n(1)])?;
            num += 1;
        }
        ret(n(num))
    })
}

/// Maintain backward compatibility for old GetNewValues function name
pub fn get_new_values(it: &mut Interp, a: Vec<Value>, want: Want) -> ER<Vec<Value>> {
    let (self_, tag, nv_hash_pt) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    get_new_value(it, vec![self_, tag, nv_hash_pt], want)
}

/// Un-escape string according to options settings and clear UTF-8 flag
/// Inputs: 0) ExifTool ref, 1) string ref or string ref ref
/// Notes: also de-references SCALAR values
pub fn sanitize(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (self_, val_pt) = (arg(&a, 0), arg(&a, 1));
    // de-reference SCALAR references
    let cur = val_pt.deref();
    if cur.ref_type() == "SCALAR" {
        pl::set_deref(&val_pt, cur.deref())?;
    }
    // make sure the Perl UTF-8 flag is OFF for the value if perl 5.6 or greater
    // (otherwise our byte manipulations get corrupted!!)
    // NOTE: Don't use Encode on Windows becase "require Encode" on Windows hangs if cwd is a long path name!!
    let mut doit = opt(&self_, "EncodeHangs").truthy();
    if !doit {
        let v = val_pt.deref();
        let r = pl::eval_block(it, |it| {
            crate::runtime::require(it, "Encode")?;
            pl::call(it, "Encode::is_utf8", vec![v])
        })?;
        doit = r.map(|x| x.truthy()).unwrap_or(false) || eval_error(it).truthy();
    }
    if doit {
        // (SIG handling was added in 10.39.  Not sure why, but I've added this to other similar code for 13.02)
        let h = set_warning_ref(it);
        pl::local_sig_warn(it, h, |it| {
            // repack by hand if Encode isn't available
            let v = val_pt.deref();
            let nv = if opt(&self_, "EncodeHangs").truthy() || eval_error(it).truthy() {
                pl::pack("C*", &pl::unpack("C0C*", &v)?)?
            } else {
                pl::call(it, "Encode::encode", vec![s("utf8"), v])?
            };
            pl::set_deref(&val_pt, nv)
        })?;
    }
    // un-escape value if necessary
    let escape = opt(&self_, "Escape");
    if escape.truthy() {
        // (XMP.pm and HTML.pm were require'd as necessary when option was set)
        if pl::str_eq(&escape, "XML") {
            let v = pl::call(it, "Image::ExifTool::XMP::UnescapeXML", vec![val_pt.deref()])?;
            pl::set_deref(&val_pt, v)?;
        } else if pl::str_eq(&escape, "HTML") {
            let v = pl::call(it, "Image::ExifTool::HTML::UnescapeHTML", vec![val_pt.deref(), opt(&self_, "Charset")])?;
            pl::set_deref(&val_pt, v)?;
        }
    }
    pl::ret_undef()
}

/// Dereference value and push onto list
/// Inputs: 0) ExifTool ref, 1) value, 2) list ref, 3) flag to push MissingTagValue for undef value
pub fn push_value(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    pl::local_topic(it, |it| {
        let (self_, val, list, missing) = (arg(&a, 0), arg(&a, 1), arg(&a, 2), arg(&a, 3));
        if val.ref_type() == "ARRAY" && val.at(0).ref_type() != "HASH" {
            for v in val.items() {
                push_value(it, vec![self_.clone(), v, list.clone(), missing.clone()], Want::Void)?;
            }
        } else if val.ref_type() == "SCALAR" {
            let d = val.deref();
            if opt(&self_, "Binary").truthy() || re(r"^Binary data", "").is_match(&d) {
                list.push(d);
            } else {
                list.push(s(&format!("Binary data {} bytes", length_of(&d))));
            }
        } else if val.ref_type() == "HASH" || val.ref_type() == "ARRAY" {
            crate::runtime::require(it, "Image/ExifTool/XMPStruct.pl")?;
            let v = pl::call(it, "Image::ExifTool::XMP::SerializeStruct", vec![self_.clone(), val])?;
            list.push(v);
        } else if val.is_undef() {
            let mval = opt(&self_, "MissingTagValue");
            if missing.truthy() && !mval.is_undef() {
                list.push(mval);
            }
        } else {
            list.push(val);
        }
        pl::ret_undef()
    })
}

/// Reformat date/time value in $_ based on specified format string
/// Inputs: 0) date/time format string
/// Returns: Reformatted date/time string
pub fn date_fmt(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let options = Value::new_hash();
    options.set("DateFormat", arg(&a, 0));
    options.set("StrictDate", n(1));
    let et = Value::new_hash();
    et.set("OPTIONS", options.clone());
    bless(&et, ET);
    let mut shift = Value::Undef;
    let adv_fmt_self = it.get_scalar(ADV_FMT_SELF);
    if adv_fmt_self.truthy() {
        shift = opt(&adv_fmt_self, "GlobalTimeShift");
        if !shift.is_undef() {
            options.set("GlobalTimeShift", shift.clone());
            et.set("GLOBAL_TIME_OFFSET", adv_fmt_self.get("GLOBAL_TIME_OFFSET"));
        }
    }
    let topic = it.get_scalar("main::_");
    let v = pl::method(it, &et, "ConvertDateTime", vec![topic])?;
    it.set_scalar("main::_", v.clone());
    if v.is_undef() {
        pl::warn(it, "Error converting date/time\n")?;
    }
    if shift.truthy() {
        let adv_fmt_self = it.get_scalar(ADV_FMT_SELF);
        if adv_fmt_self.is_hash() {
            adv_fmt_self.set("GLOBAL_TIME_OFFSET", et.get("GLOBAL_TIME_OFFSET"));
        }
    }
    ret(it.get_scalar("main::_"))
}

/// Utility routine to remove duplicate items from default input string
/// Inputs: 0) true to set $_ to undef if not changed
/// Notes: - for use only in advanced formatting expressions
pub fn no_dups(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let mut seen = std::collections::HashSet::new();
    let adv_fmt_self = it.get_scalar(ADV_FMT_SELF);
    let sep = if adv_fmt_self.truthy() { opt(&adv_fmt_self, "ListSep") } else { s(", ") };
    let topic = it.get_scalar("main::_");
    let r = pl::re_dyn(&crate::perl::regex::quotemeta(&sep.to_bytes()), "")?;
    let items: Vec<Vec<u8>> = pl::split(&r, &topic, 0).into_iter().map(|x| x.to_bytes().to_vec()).filter(|x| seen.insert(x.clone())).collect();
    let new = Value::bytes(items.join(&sep.to_bytes()[..]));
    let v = if arg(&a, 0).truthy() && eq(&new, &topic) { Value::Undef } else { new };
    it.set_scalar("main::_", v.clone());
    ret(v)
}

/// Utility routine to set in $_ image from current object
/// Inputs: 0-N) list of tags to copy
/// Returns: Return value from WriteInfo
/// Notes: - for use only in advanced formatting expressions
pub fn set_tags(it: &mut Interp, a: Vec<Value>, want: Want) -> ER<Vec<Value>> {
    let self_ = it.get_scalar(ADV_FMT_SELF);
    let et = pl::method(it, &s(ET), "new", vec![])?;
    let mut args = vec![self_];
    args.extend(a);
    method_void(it, &et, "SetNewValuesFromFile", args)?;
    let topic = match it.global(b'$', "main::_") {
        Binding::Scalar(c) => Value::ScalarRef(c),
        _ => unreachable!(),
    };
    it.call_method(&et, "WriteInfo", vec![et.clone(), topic], want)
}

/// Is specified tag writable
/// Inputs: 0) tag name, case insensitive (optional group name currently ignored)
/// Returns: 0=exists but not writable, 1=writable, undef=doesn't exist
pub fn is_writable(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let mut tag = arg(&a, 0);
    re(r"^(.*):", "").subst(&mut tag, false, |_| Vec::new()); // ignore group name
    let tag_info_list = pl::call_list(it, "Image::ExifTool::TagLookup::FindTagInfo", vec![tag.clone()])?;
    if tag_info_list.is_empty() {
        if pl::call(it, "Image::ExifTool::TagLookup::TagExists", vec![tag])?.truthy() {
            return ret(n(0));
        }
        return pl::ret_undef();
    }
    for tag_info in tag_info_list {
        let writable = tag_info.get("Writable");
        if !writable.is_undef() {
            return ret(if writable.truthy() { n(1) } else { n(0) });
        }
        let table = viv(&tag_info, "Table");
        if table.get("WRITABLE").truthy() {
            return ret(n(1));
        }
        // must call WRITE_PROC to autoload writer because this may set the writable tag
        let write_proc = table.get("WRITE_PROC");
        if write_proc.truthy() {
            pl::call_code(it, &write_proc, vec![], Want::Scalar)?; // dummy call to autoload writer
            if tag_info.get("Writable").truthy() {
                return ret(n(1));
            }
        }
    }
    ret(n(0))
}

/// Check to see if these are the same file
/// Inputs: 0) ExifTool ref, 1) first file name, 2) second file name
/// Returns: true if file names reference the same file
pub fn is_same_file(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (self_, file, file2) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    if !eq(&lc(&file), &lc(&file2)) {
        return ret(n(0)); // (only looking for differences in case)
    }
    let mut is_same = Value::Undef;
    let interrupted = Rc::new(RefCell::new(false));
    let pid = std::process::id();
    let tmp1 = pl::concat(&file, &s(&format!("_ExifTool_tmp_{pid}")));
    let tmp2 = pl::concat(&file2, &s(&format!("_ExifTool_tmp_{pid}")));
    {
        // local *TMP1;
        // local $SIG{INT} = sub { $interrupted = 1 };
        let sig = pl::pkg_hash(it, "main::SIG");
        let old = sig.exists("INT").then(|| sig.get("INT"));
        let flag = interrupted.clone();
        sig.set(
            "INT",
            closure("Image::ExifTool::__ANON__", move |_it, _a, _w| {
                *flag.borrow_mut() = true;
                Ok(vec![n(1)])
            }),
        );
        let r = (|| -> ER<()> {
            let fh = glob_ref(it, "TMP1")?;
            if pl::method(it, &self_, "Open", vec![fh, tmp1.clone(), s(">")])?.truthy() {
                perl_snippet(it, "close TMP1", &[], Want::Void)?;
                if pl::method(it, &self_, "Exists", vec![tmp2.clone()])?.truthy() {
                    is_same = n(1);
                }
                method_void(it, &self_, "Unlink", vec![tmp1.clone()])?;
            }
            Ok(())
        })();
        let sig = pl::pkg_hash(it, "main::SIG");
        match old {
            Some(v) => sig.set("INT", v),
            None => {
                sig.delete("INT");
            }
        }
        r?;
    }
    let sig_int = pl::pkg_hash(it, "main::SIG").get("INT");
    if *interrupted.borrow() && sig_int.truthy() {
        pl::call_code(it, &sig_int, vec![], Want::Void)?;
    }
    ret(is_same)
}

/// Copy file attributes from one file to another
/// Inputs: 0) ExifTool ref, 1) source file name, 2) destination file name
/// Notes: eventually add support for extended attributes?
pub fn copy_file_attrs(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (self_, src, dst) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    let st = perl_snippet(it, "(stat($src))[2, 4, 5]", &[("src", src)], Want::List)?.0;
    let (mode, mut uid, mut gid) = (arg(&st, 0), arg(&st, 1), arg(&st, 2));
    // copy file attributes unless we already set them
    if !mode.is_undef() && get_new_value_s(it, &self_, s("FilePermissions"))?.is_undef() {
        perl_snippet(it, "eval { chmod($mode & 07777, $dst) }", &[("mode", mode), ("dst", dst.clone())], Want::Void)?;
    }
    let new_uid = get_new_value_s(it, &self_, s("FileUserID"))?;
    let new_gid = get_new_value_s(it, &self_, s("FileGroupID"))?;
    if !uid.is_undef() && !gid.is_undef() && (new_uid.is_undef() || new_gid.is_undef()) {
        if !new_gid.is_undef() {
            gid = new_gid;
        }
        if !new_uid.is_undef() {
            uid = new_uid;
        }
        perl_snippet(it, "eval { chown($uid, $gid, $dst) }", &[("uid", uid), ("gid", gid), ("dst", dst)], Want::Void)?;
    }
    pl::ret_undef()
}

/// `$self->GetNewValue($tag)` in scalar context.
fn get_new_value_s(it: &mut Interp, self_: &Value, tag: Value) -> ER<Value> {
    Ok(get_new_value(it, vec![self_.clone(), tag], Want::Scalar)?.pop().unwrap_or_default())
}

/// `$self->GetNewValue($tag, \$nvHash)` in scalar context: the value, and
/// the new value hash.
fn get_new_value_nvh(it: &mut Interp, self_: &Value, tag: Value, nv_hash: &mut Value) -> ER<Value> {
    let pt = pl::sref(nv_hash.clone());
    let v = get_new_value(it, vec![self_.clone(), tag, pt.clone()], Want::Scalar)?.pop().unwrap_or_default();
    *nv_hash = pt.deref();
    Ok(v)
}

/// Reverse hash lookup
/// Inputs: 0) value, 1) hash reference
/// Returns: Hash key or undef if not found (plus flag for multiple matches in list context)
pub fn reverse_lookup(it: &mut Interp, a: Vec<Value>, want: Want) -> ER<Vec<Value>> {
    let (mut val, conv) = (arg(&a, 0), arg(&a, 1));
    if val.is_undef() {
        return pl::ret_undef();
    }
    let mut multi = Value::Undef;
    if let Some(c) = re(r"^Unknown\s*\((.*)\)$", "i").m(&val) {
        val = cap(&c, 1); // was unknown
        if let Some(c) = re(r"^0x([\da-fA-F]+)$", "").m(&val) {
            // disable "Hexadecimal number > 0xffffffff non-portable" warning
            val = hex(&cap(&c, 1)); // convert hex value
        }
    } else {
        let mut qval = val.clone();
        re(r"\s+$", "").subst(&mut qval, false, |_| Vec::new()); // remove trailing whitespace
        let q = crate::perl::regex::quotemeta(&qval.to_bytes());
        let mk = |pre: &str, post: &str| -> Vec<u8> {
            let mut v = pre.as_bytes().to_vec();
            v.extend_from_slice(&q);
            v.extend_from_slice(post.as_bytes());
            v
        };
        let patterns = [
            mk("^", "$"),     // exact match
            mk("^(?i)", "$"), // case-insensitive
            mk("^(?i)", ""),  // beginning of string
            mk("(?i)", ""),   // substring
        ];
        // hash entries to ignore in reverse lookup
        let ignore_print_conv = my_hash(it, "ignorePrintConv");
        let mut found = false;
        let mut matches: i64 = 0;
        'pat: for pattern in patterns.iter() {
            let r = pl::re_dyn(pattern, "")?;
            matches = values(&conv).iter().filter(|v| r.is_match(v)).count() as i64;
            if matches == 0 {
                continue;
            }
            // multiple matches are bad unless they were exact
            if matches > 1 && !pattern.ends_with(b"$") {
                // don't match entries that we should ignore
                for k in keys(&ignore_print_conv) {
                    let cv = conv.get(k.to_bytes());
                    if !cv.is_undef() && r.is_match(&cv) {
                        matches -= 1;
                    }
                }
                if matches > 1 {
                    break;
                }
            }
            for k in sort_str(keys(&conv)) {
                if !r.is_match(&conv.get(k.to_bytes())) || ignore_print_conv.get(k.to_bytes()).truthy() {
                    continue;
                }
                val = k;
                found = true;
                break 'pat;
            }
        }
        if !found {
            // call OTHER conversion routine if available
            let other = conv.get("OTHER");
            if other.truthy() {
                let h = set_warning_ref(it);
                val = pl::local_sig_warn(it, h, |it| {
                    it.set_scalar(EVAL_WARNING, Value::Undef);
                    Ok(pl::call_code(it, &other, vec![val.clone(), n(1), conv.clone()], Want::Scalar)?.pop().unwrap_or_default())
                })?;
            } else {
                val = Value::Undef;
            }
            if matches > 1 {
                multi = n(1);
            }
        }
    }
    if want == Want::List {
        return Ok(vec![val, multi]);
    }
    ret(val)
}

/// Get list of tags to write for Geolocate feature
/// Inputs: 0) ExifTool ref, 1) group name(s),
///         2) 0=prefer writing City, 1=prefer writing GPS, undef=deleting tags
/// Returns: list of tags to write/delete
pub fn get_geolocate_tags(_it: &mut Interp, a: Vec<Value>, want: Want) -> ER<Vec<Value>> {
    let (want_group, mut write_gps) = (arg(&a, 1), arg(&a, 2));
    let mut grps: Vec<Value> = if want_group.truthy() { split_on(":", &want_group).iter().map(lc).collect() } else { Vec::new() };
    let grps_h = Value::new_hash(); // lookup for specified groups
    for g in &grps {
        grps_h.set(g.to_bytes(), g.clone());
    }
    if grps_h.get("exif").truthy() && !grps_h.get("gps").truthy() {
        grps_h.set("gps", s("gps"));
        grps.push(s("gps"));
    }
    let has = |k: &str| grps_h.get(k).truthy();
    let tag_groups: &[(&str, &[&str])] = &[
        ("xmp-iptcext", &["LocationShownCity", "LocationShownProvinceState", "LocationShownCountryCode", "LocationShownCountryName", "LocationShownGPSLatitude", "LocationShownGPSLongitude"]),
        ("xmp-photoshop", &["City", "State", "Country"]),
        ("xmp-iptccore", &["CountryCode"]),
        ("iptc", &["City", "Province-State", "Country-PrimaryLocationCode", "Country-PrimaryLocationName"]),
        ("gps", &["GPSLatitude", "GPSLongitude", "GPSLatitudeRef", "GPSLongitudeRef"]),
        ("xmp-exif", &["GPSLatitude", "GPSLongitude"]),
        ("itemlist", &["GPSCoordinates"]),
        ("userdata", &["GPSCoordinates"]),
        // more general groups not in this lookup: XMP and QuickTime
    ];
    let mut tags: Vec<Value> = Vec::new();
    // set specific City and GPS tags
    for grp in &grps {
        if let Some((_, list)) = tag_groups.iter().find(|(k, _)| pl::str_eq(grp, k)) {
            for t in list.iter() {
                tags.push(pl::concat(&pl::concat(grp, &s(":")), &s(t)));
            }
        }
    }
    let push = |tags: &mut Vec<Value>, l: &[&str]| tags.extend(l.iter().map(|x| s(x)));
    // set default XMP City tags if necessary
    if !write_gps.truthy() {
        if has("keys") {
            push(&mut tags, &["Keys:LocationName"]);
        }
        if has("xmp") || (tags.is_empty() && !has("quicktime")) {
            push(&mut tags, &["XMP:City", "XMP:State", "XMP:CountryCode", "XMP:Country", "Keys:LocationName"]);
        }
    }
    if write_gps.is_undef() {
        write_gps = n(1); // (delete both City and GPS)
    }
    if write_gps.truthy() && has("keys") {
        push(&mut tags, &["Keys:GPSCoordinates"]);
    }
    // set default QuickTime tag if necessary
    let did_qt = tags.iter().filter(|t| re(r"GPSCoordinates$", "").is_match(t)).count();
    if (has("quicktime") && did_qt == 0) || (write_gps.truthy() && tags.is_empty() && !has("xmp")) {
        push(&mut tags, &["QuickTime:GPSCoordinates"]);
    }
    // set default GPS tags if necessary
    if write_gps.truthy() {
        if has("xmp") && !has("xmp-exif") {
            push(&mut tags, &["XMP:GPSLatitude", "XMP:GPSLongitude"]);
        }
        if !want_group.truthy() {
            push(&mut tags, &["GPSLatitude", "GPSLongitude", "GPSLatitudeRef", "GPSLongitudeRef"]);
        }
    }
    ret_count(tags, want)
}

/// Get new value hash for specified tagInfo/writeGroup
/// Inputs: 0) ExifTool object reference, 1) reference to tag info hash
///         2) Write group name, 3) Options: 'delete' or 'create' new value hash
///         4) optional ProtectSaved value, 5) true if we are deleting a value
/// Returns: new value hash reference for specified write group
///          (or first new value hash in linked list if write group not specified)
/// Notes: May return undef when 'create' is used with ProtectSaved
pub fn get_new_value_hash(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (self_, tag_info, mut write_group, opts) = (arg(&a, 0), arg(&a, 1), arg(&a, 2), arg(&a, 3));
    if !tag_info.truthy() {
        return pl::ret_undef();
    }
    let tik = tag_info.to_bytes();
    let mut nv_hash = viv(&self_, "NEW_VALUE").get(&tik);

    // quick lookup for options
    let opt_delete = opts.truthy() && pl::str_eq(&opts, "delete");
    let opt_create = opts.truthy() && pl::str_eq(&opts, "create");
    if write_group.is_undef() {
        write_group = s("");
    }

    if write_group.truthy() {
        // find the new value in the list with the specified write group
        while nv_hash.truthy() && !eq(&nv_hash.get("WriteGroup"), &write_group) {
            // QuickTime and All are special cases because all group1 tags may be updated at once
            if re(r"^(QuickTime|All)$", "").is_match(&nv_hash.get("WriteGroup")) {
                break;
            }
            // replace existing entry if WriteGroup is 'All' (avoids confusion of forum10349)
            //last if $$tagInfo{WriteGroup} and $$tagInfo{WriteGroup} eq 'All'; # (didn't work for forum17770)
            // forum17770 patch (also handles case where "EXIF" is specified as a write group)
            if pl::str_eq(&write_group, "All") || (pl::str_eq(&nv_hash.get("WriteGroup"), "EXIF") && re(r"IFD", "").is_match(&write_group)) {
                break;
            }
            nv_hash = nv_hash.get("Next");
        }
    }
    // remove this entry if deleting, or if creating a new entry and
    // this entry is marked with "Save" flag
    if !nv_hash.is_undef() && (opt_delete || (opt_create && nv_hash.get("Save").truthy())) {
        let a4 = arg(&a, 4);
        let protect = !a4.is_undef() && !nv_hash.get("Save").is_undef() && pl::gt(&nv_hash.get("Save"), &a4);
        // this is a bit tricky:  we want to add to a protected nvHash only if we
        // are adding a conditional delete ($_[5] true or DelValue with no Shift)
        // or accumulating List items (NoReplace true)
        // (NOTE: this should be looked into --> lists may be accumulated instead of being replaced
        // as expected when copying to the same list from different dynamic -tagsFromFile source files)
        if protect && !(opt_create && (nv_hash.get("NoReplace").truthy() || arg(&a, 5).truthy() || (nv_hash.get("DelValue").truthy() && nv_hash.get("Shift").is_undef()))) {
            return pl::ret_undef(); // honour ProtectSaved value by not writing this tag
        } else if opt_delete {
            remove_new_value_hash(it, vec![self_.clone(), nv_hash.clone(), tag_info.clone()], Want::Void)?;
            nv_hash = Value::Undef;
        } else {
            // save a copy of this new value hash
            let copy = nv_hash.hash_copy();
            // make copy of Value and DelValue lists
            for key in keys(&copy) {
                let v = copy.get(key.to_bytes());
                if v.ref_type() != "ARRAY" {
                    continue;
                }
                copy.set(key.to_bytes(), Value::new_array(v.items()));
            }
            let mut save_hash = self_.get("SAVE_NEW_VALUE");
            if save_hash.is_undef() {
                save_hash = Value::new_hash(); // (perl vivifies the lexical)
            }
            // add to linked list of saved new value hashes
            copy.set("Next", save_hash.get(&tik));
            save_hash.set(&tik, copy);
            nv_hash.delete("Save"); // don't save it again
            if protect && nv_hash.get("Value").truthy() {
                nv_hash.set("AddBefore", n(nv_hash.get("Value").len() as i64));
            }
        }
    }
    if nv_hash.is_undef() && opt_create {
        // create a new entry
        nv_hash = Value::new_hash();
        nv_hash.set("TagInfo", tag_info.clone());
        nv_hash.set("WriteGroup", write_group);
        nv_hash.set("IsNVH", n(1)); // set flag so we can recognize a new value hash
        nv_hash.set("Order", post_inc(&self_, "NV_COUNT"));
        // add entry to our NEW_VALUE hash
        let nv = viv(&self_, "NEW_VALUE");
        if nv.get(&tik).truthy() {
            // add to end of linked list
            let last_hash = pl::call(it, "Image::ExifTool::LastInList", vec![nv.get(&tik)])?;
            last_hash.set("Next", nv_hash.clone());
        } else {
            nv.set(&tik, nv_hash.clone());
        }
    }
    ret(nv_hash)
}

/// `$self->GetNewValueHash(ARGS)` in scalar context.
fn gnvh(it: &mut Interp, args: Vec<Value>) -> ER<Value> {
    Ok(get_new_value_hash(it, args, Want::Scalar)?.pop().unwrap_or_default())
}

/// Load all tag tables
pub fn load_all_tables(it: &mut Interp, _a: Vec<Value>, want: Want) -> ER<Vec<Value>> {
    let loaded = my_scalar(it, "loadedAllTables");
    if loaded.borrow().truthy() {
        return pl::ret_nothing(want);
    }

    // load all of our non-referenced tables (first our modules)
    for table in pl::pkg_array(it, "Image::ExifTool::loadAllTables").items() {
        let mut table_name = pl::concat(&s("Image::ExifTool::"), &table);
        if !re(r":", "").is_match(&table) {
            table_name = pl::concat(&table_name, &s("::Main"));
        }
        pl::call(it, "Image::ExifTool::GetTagTable", vec![table_name])?;
    }
    // (then our special tables)
    pl::call(it, "Image::ExifTool::GetTagTable", vec![s("Image::ExifTool::Extra")])?;
    pl::call(it, "Image::ExifTool::GetTagTable", vec![s("Image::ExifTool::Composite")])?;
    // recursively load all tables referenced by the current tables
    let all_tables = pl::pkg_hash(it, "Image::ExifTool::allTables");
    let mut table_names: std::collections::VecDeque<Value> = keys(&all_tables).into();
    let mut pushed_tables = std::collections::HashSet::new();
    while let Some(name) = table_names.pop_front() {
        let table = pl::call(it, "Image::ExifTool::GetTagTable", vec![name])?;
        // call write proc if it exists in case it adds tags to the table
        let write_proc = table.get("WRITE_PROC");
        if write_proc.truthy() {
            pl::call_code(it, &write_proc, vec![], Want::Scalar)?;
        }
        // recursively scan through tables in subdirectories
        for k in pl::call_list(it, "Image::ExifTool::TagTableKeys", vec![table.clone()])? {
            let info_array = pl::call_list(it, "Image::ExifTool::GetTagInfoList", vec![table.clone(), k])?;
            for tag_info in info_array {
                let subdir = tag_info.get("SubDirectory");
                if !subdir.truthy() {
                    continue;
                }
                let table_name = subdir.get("TagTable");
                if !table_name.truthy() {
                    continue;
                }
                // next if table already loaded or queued for loading
                let tn = table_name.to_bytes().to_vec();
                if all_tables.get(&tn).truthy() || pushed_tables.contains(&tn) {
                    continue;
                }
                table_names.push_back(table_name); // must scan this one too
                pushed_tables.insert(tn);
            }
        }
    }
    *loaded.borrow_mut() = n(1);
    ret(n(1))
}

/// Remove new value hash from linked list (and save if necessary)
/// Inputs: 0) ExifTool object reference, 1) new value hash ref, 2) tagInfo ref
pub fn remove_new_value_hash(_it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (self_, nv_hash, tag_info) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    let tik = tag_info.to_bytes();
    let new_value = viv(&self_, "NEW_VALUE");
    let mut first_hash = new_value.get(&tik);
    if eq(&nv_hash, &first_hash) {
        // remove first entry from linked list
        if nv_hash.get("Next").truthy() {
            new_value.set(&tik, nv_hash.get("Next"));
        } else {
            new_value.delete(&tik);
        }
    } else {
        // find the list element pointing to this hash
        loop {
            if first_hash.is_undef() {
                first_hash = Value::new_hash(); // (perl vivifies the lexical)
            }
            if eq(&first_hash.get("Next"), &nv_hash) {
                break;
            }
            first_hash = first_hash.get("Next");
        }
        // remove from linked list
        first_hash.set("Next", nv_hash.get("Next"));
    }
    // save the existing entry if necessary
    if nv_hash.get("Save").truthy() {
        let mut save_hash = self_.get("SAVE_NEW_VALUE");
        if save_hash.is_undef() {
            save_hash = Value::new_hash(); // (perl vivifies the lexical)
        }
        // add to linked list of saved new value hashes
        nv_hash.set("Next", save_hash.get(&tik));
        save_hash.set(&tik, nv_hash.clone());
        return ret(nv_hash);
    }
    pl::ret_undef()
}

/// Remove all new value entries for specified group
/// Inputs: 0) ExifTool object reference, 1) group name
pub fn remove_new_values_for_group(it: &mut Interp, a: Vec<Value>, want: Want) -> ER<Vec<Value>> {
    let (self_, group) = (arg(&a, 0), arg(&a, 1));

    if !self_.get("NEW_VALUE").truthy() {
        return pl::ret_nothing(want);
    }

    // make list of all groups we must remove
    let mut groups = vec![group.clone()];
    let remove_groups = my_hash(it, "removeGroups");
    let rg = remove_groups.get(group.to_bytes());
    if rg.truthy() {
        groups.extend(rg.items());
    }

    let mut out = Value::Undef;
    if pl::gt(&opt(&self_, "Verbose"), &n(1)) {
        out = opt(&self_, "TextOut");
    }

    // loop though all new values, and remove any in this group
    let ks = keys(&viv(&self_, "NEW_VALUE"));
    for hash_key in ks {
        let mut nv_hash = viv(&self_, "NEW_VALUE").get(hash_key.to_bytes());
        // loop through each entry in linked list
        loop {
            let next_hash = nv_hash.get("Next");
            let tag_info = nv_hash.get("TagInfo");
            let g = pl::method_list(it, &self_, "GetGroup", vec![tag_info.clone()])?;
            let (grp0, grp1) = (arg(&g, 0), arg(&g, 1));
            let mut wgrp = nv_hash.get("WriteGroup");
            // use group1 if write group is not specific
            if eq(&wgrp, &grp0) {
                wgrp = grp1;
            }
            let mut p = b"^(".to_vec();
            p.extend_from_slice(&grp0.to_bytes());
            p.push(b'|');
            p.extend_from_slice(&wgrp.to_bytes());
            p.extend_from_slice(b")$");
            if pl::str_eq(&grp0, "*") || pl::str_eq(&wgrp, "*") || !grep_dyn(&p, "i", &groups)?.is_empty() {
                if out.truthy() {
                    print_out(it, &out, cat(&[&s("Removed new value for "), &wgrp, &s(":"), &tag_info.get("Name"), &s("\n")]))?;
                }
                // remove from linked list
                remove_new_value_hash(it, vec![self_.clone(), nv_hash.clone(), tag_info.clone()], Want::Void)?;
            }
            nv_hash = next_hash;
            if !nv_hash.truthy() {
                break;
            }
        }
    }
    pl::ret_undef()
}

/// Get list of tagInfo hashes for all new data
/// Inputs: 0) ExifTool object reference, 1) optional tag table pointer
/// Returns: list of tagInfo hashes in no particular order
pub fn get_new_tag_info_list(_it: &mut Interp, a: Vec<Value>, want: Want) -> ER<Vec<Value>> {
    let (self_, tag_table_ptr) = (arg(&a, 0), arg(&a, 1));
    let mut tag_info_list = Vec::new();
    let nv = self_.get("NEW_VALUE");
    if nv.truthy() {
        for hash_key in keys(&nv) {
            let tag_info = viv(&nv, hash_key.to_bytes()).get("TagInfo");
            if tag_table_ptr.truthy() && !eq(&tag_table_ptr, &tag_info.get("Table")) {
                continue;
            }
            tag_info_list.push(tag_info);
        }
    }
    ret_count(tag_info_list, want)
}

/// Get hash of tagInfo references keyed on tagID for a specific table
/// Inputs: 0) ExifTool object reference, 1-N) tag table pointers
/// Returns: hash reference
/// Notes: returns only one tagInfo ref for each conditional list
pub fn get_new_tag_info_hash(_it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let self_ = arg(&a, 0);
    let tag_info_hash = Value::new_hash();
    let nv = self_.get("NEW_VALUE");
    let mut i = 1;
    while nv.truthy() {
        let tag_table_ptr = arg(&a, i);
        i += 1;
        if !tag_table_ptr.truthy() {
            break;
        }
        for hash_key in keys(&nv) {
            let tag_info = viv(&nv, hash_key.to_bytes()).get("TagInfo");
            if tag_table_ptr.truthy() && !eq(&tag_table_ptr, &tag_info.get("Table")) {
                continue;
            }
            tag_info_hash.set(tag_info.get("TagID").to_bytes(), tag_info);
        }
    }
    ret(tag_info_hash)
}

/// Get a tagInfo/tagID hash for subdirectories we need to add
/// Inputs: 0) ExifTool object reference, 1) parent tag table reference
///         2) parent directory name (taken from GROUP0 of tag table if not defined)
/// Returns: Reference to Hash of subdirectory tagInfo references keyed by tagID
///          (plus Reference to edit directory hash in list context)
pub fn get_add_dir_hash(it: &mut Interp, a: Vec<Value>, want: Want) -> ER<Vec<Value>> {
    let (self_, tag_table_ptr, mut parent) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    if !parent.truthy() {
        parent = viv(&tag_table_ptr, "GROUPS").get("0");
    }
    let add_dir_hash = Value::new_hash();
    let edit_dir_hash = Value::new_hash();
    let add_dirs = self_.get("ADD_DIRS");
    let edit_dirs = self_.get("EDIT_DIRS");
    for tag_id in pl::call_list(it, "Image::ExifTool::TagTableKeys", vec![tag_table_ptr.clone()])? {
        let info_array = pl::call_list(it, "Image::ExifTool::GetTagInfoList", vec![tag_table_ptr.clone(), tag_id.clone()])?;
        for tag_info in info_array {
            if !tag_info.get("SubDirectory").truthy() {
                continue;
            }
            // get name for this sub directory
            // (take directory name from SubDirectory DirName if it exists,
            //  otherwise Group0 name of SubDirectory TagTable or tag Group1 name)
            let mut dir_name = viv(&tag_info, "SubDirectory").get("DirName");
            if !dir_name.truthy() {
                // use tag name for directory name and save for next time
                dir_name = tag_info.get("Name");
                viv(&tag_info, "SubDirectory").set("DirName", dir_name.clone());
            }
            // save this directory information if we are writing it
            let ed = edit_dirs.get(dir_name.to_bytes());
            if ed.truthy() && eq(&ed, &parent) {
                edit_dir_hash.set(tag_id.to_bytes(), tag_info.clone());
                if add_dirs.get(dir_name.to_bytes()).truthy() {
                    add_dir_hash.set(tag_id.to_bytes(), tag_info.clone());
                }
            }
        }
    }
    if want == Want::List {
        return Ok(vec![add_dir_hash, edit_dir_hash]);
    }
    ret(add_dir_hash)
}

/// Get localized version of tagInfo hash (used by MIE, XMP, PNG and QuickTime)
/// Inputs: 0) tagInfo hash ref, 1) locale code (eg. "en_CA" for MIE)
/// Returns: new tagInfo hash ref, or undef if invalid
/// - sets LangCode member in new tagInfo
pub fn get_lang_info(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (tag_info, lang_code) = (arg(&a, 0), arg(&a, 1));
    // make a new tagInfo hash for this locale
    let table = tag_info.get("Table");
    let tag_id = cat(&[&tag_info.get("TagID"), &s("-"), &lang_code]);
    let mut lang_info = table.get(tag_id.to_bytes());
    if !lang_info.truthy() {
        // make a new tagInfo entry for this locale
        lang_info = hash_rv(&tag_info)?.hash_copy();
        lang_info.set("Name", cat(&[&tag_info.get("Name"), &s("-"), &lang_code]));
        let desc = pl::call(it, "Image::ExifTool::MakeDescription", vec![tag_info.get("Name")])?;
        lang_info.set("Description", cat(&[&desc, &s(" ("), &lang_code, &s(")")]));
        lang_info.set("LangCode", lang_code.clone());
        lang_info.set("SrcTagInfo", tag_info.clone()); // save reference to original tagInfo
        call_void(it, "Image::ExifTool::AddTagToTable", vec![table, tag_id, lang_info.clone()])?;
    }
    ret(lang_info)
}

/// `@tg` matches every group of `groups` (`grep /^$group$/i, @tg`).
fn groups_match(groups: &[Value], tg: &[Value]) -> ER<bool> {
    for group in groups {
        if grep_dyn(&anchored(group), "i", tg)?.is_empty() {
            return Ok(false);
        }
    }
    Ok(true)
}

/// Get list of all available tags for specified group
/// Inputs: 0) optional group name (or string of names separated by colons)
/// Returns: tag list (sorted alphabetically)
/// Notes: Can't get tags for specific IFD
pub fn get_all_tags(it: &mut Interp, a: Vec<Value>, want: Want) -> ER<Vec<Value>> {
    pl::local_topic(it, |it| {
        let group = arg(&a, 0);
        let all_tags = Value::new_hash();
        let groups = if group.truthy() { split_on(":", &group) } else { Vec::new() };

        let et = pl::method(it, &s(ET), "new", vec![])?;
        load_all_tables(it, vec![], Want::Void)?; // first load all our tables
        let mut table_names = keys(&pl::pkg_hash(it, "Image::ExifTool::allTables"));

        // loop through all tables and save tag names to %allTags hash
        while let Some(name) = table_names.pop() {
            let table = pl::call(it, "Image::ExifTool::GetTagTable", vec![name])?;
            // generate flattened tag names for structure fields if this is an XMP table
            if table.get("GROUPS").truthy() && pl::str_eq(&viv(&table, "GROUPS").get("0"), "XMP") {
                call_void(it, "Image::ExifTool::XMP::AddFlattenedTags", vec![table.clone()])?;
            }
            for tag_id in pl::call_list(it, "Image::ExifTool::TagTableKeys", vec![table.clone()])? {
                let info_array = pl::call_list(it, "Image::ExifTool::GetTagInfoList", vec![table.clone(), tag_id])?;
                for tag_info in info_array {
                    let tag = tag_info.get("Name");
                    if !tag.truthy() {
                        pl::warn(it, "no name for tag!\n")?;
                        continue;
                    }
                    // don't list subdirectories unless they are writable
                    if tag_info.get("SubDirectory").truthy() && !tag_info.get("Writable").truthy() {
                        continue;
                    }
                    if tag_info.get("Hidden").truthy() {
                        continue; // ignore hidden tags
                    }
                    if !groups.is_empty() {
                        let tg = pl::method_list(it, &et, "GetGroup", vec![tag_info.clone()])?;
                        if !groups_match(&groups, &tg)? {
                            continue;
                        }
                    }
                    all_tags.set(tag.to_bytes(), n(1));
                }
            }
        }
        ret_sorted(sort_str(keys(&all_tags)), want)
    })
}

/// Get list of all writable tags
/// Inputs: 0) optional group name (or names separated by colons)
/// Returns: tag list (sorted alphabetically)
pub fn get_writable_tags(it: &mut Interp, a: Vec<Value>, want: Want) -> ER<Vec<Value>> {
    pl::local_topic(it, |it| {
        let group = arg(&a, 0);
        let writable_tags = Value::new_hash();
        let groups = if group.truthy() { split_on(":", &group) } else { Vec::new() };

        let et = pl::method(it, &s(ET), "new", vec![])?;
        load_all_tables(it, vec![], Want::Void)?;
        let mut table_names = keys(&pl::pkg_hash(it, "Image::ExifTool::allTables"));

        while let Some(table_name) = table_names.pop() {
            let table = pl::call(it, "Image::ExifTool::GetTagTable", vec![table_name.clone()])?;
            // generate flattened tag names for structure fields if this is an XMP table
            if table.get("GROUPS").truthy() && pl::str_eq(&viv(&table, "GROUPS").get("0"), "XMP") {
                call_void(it, "Image::ExifTool::XMP::AddFlattenedTags", vec![table.clone()])?;
            }
            // attempt to load Write tables if autoloaded
            let mut parts = split_on("::", &table_name);
            if parts.len() > 3 {
                let i = parts.len() - 2;
                parts[i] = pl::concat(&s("Write"), &parts[i]); // add 'Write' before class name
                let module = pl::join("::", &parts[0..=i]);
                // (fails silently if nothing loaded)
                pl::eval_block(it, |it| perl_snippet(it, "require $module", &[("module", module.clone())], Want::Scalar))?;
            }
            for tag_id in pl::call_list(it, "Image::ExifTool::TagTableKeys", vec![table.clone()])? {
                let info_array = pl::call_list(it, "Image::ExifTool::GetTagInfoList", vec![table.clone(), tag_id])?;
                for tag_info in info_array {
                    let tag = tag_info.get("Name");
                    if !tag.truthy() {
                        pl::warn(it, "no name for tag!\n")?;
                        continue;
                    }
                    let writable = tag_info.get("Writable");
                    if !(writable.truthy() || (table.get("WRITABLE").truthy() && writable.is_undef() && !tag_info.get("SubDirectory").truthy())) {
                        continue;
                    }
                    if tag_info.get("Hidden").truthy() {
                        continue; // ignore hidden tags
                    }
                    if !groups.is_empty() {
                        let tg = pl::method_list(it, &et, "GetGroup", vec![tag_info.clone()])?;
                        if !groups_match(&groups, &tg)? {
                            continue;
                        }
                    }
                    writable_tags.set(tag.to_bytes(), n(1));
                }
            }
        }
        ret_sorted(sort_str(keys(&writable_tags)), want)
    })
}

/// Get list of all group names
/// Inputs: 0) [optional] ExifTool ref, 1) Group family number
/// Returns: List of group names (sorted alphabetically)
pub fn get_all_groups(it: &mut Interp, a: Vec<Value>, want: Want) -> ER<Vec<Value>> {
    pl::local_topic(it, |it| {
        let mut args = a.clone().into_iter();
        let or0 = |v: Option<Value>| match v {
            Some(v) if v.truthy() => v,
            _ => n(0),
        };
        let mut family = or0(args.next());
        let mut self_ = Value::Undef;
        if family.is_ref() {
            self_ = family;
            family = or0(args.next());
        }

        if pl::num_eq(&family, &n(3)) {
            return ret_list(vec![s("Doc#"), s("Main")], want);
        }
        if pl::num_eq(&family, &n(4)) {
            return ret_list(vec![s("Copy#")], want);
        }
        if pl::num_eq(&family, &n(5)) {
            return ret_list(vec![s("[too many possibilities to list]")], want);
        }
        if pl::num_eq(&family, &n(6)) {
            let fnum = pl::pkg_hash(it, "Image::ExifTool::Exif::formatNumber");
            let mut k = keys(&fnum);
            k.sort_by(|x, y| ncmp(&fnum.get(x.to_bytes()), &fnum.get(y.to_bytes())));
            return ret_sorted(k, want);
        }
        if pl::num_eq(&family, &n(8)) {
            return ret_list(vec![s("File#")], want);
        }

        load_all_tables(it, vec![], Want::Void)?; // first load all our tables

        let mut table_names = keys(&pl::pkg_hash(it, "Image::ExifTool::allTables"));

        let all_groups = Value::new_hash();
        // add family 1 groups not in tables
        // start with family 1 groups that are missing from the tables
        if pl::num_eq(&family, &n(1)) {
            for g in ["Garmin", "AudioItemList", "AudioUserData", "VideoItemList", "VideoUserData", "Track#Keys", "Track#ItemList", "Track#UserData", "KFIX"] {
                all_groups.set(g, n(1));
            }
        }
        let fk = family.to_bytes();
        // loop through all tag tables and get all group names
        while let Some(name) = table_names.pop() {
            let table = pl::call(it, "Image::ExifTool::GetTagTable", vec![name])?;
            let grps = table.get("GROUPS");
            if grps.truthy() {
                let grp = grps.get(&fk);
                if grp.truthy() {
                    all_groups.set(grp.to_bytes(), n(1));
                }
            }
            for tag in pl::call_list(it, "Image::ExifTool::TagTableKeys", vec![table.clone()])? {
                let info_array = pl::call_list(it, "Image::ExifTool::GetTagInfoList", vec![table.clone(), tag])?;
                if pl::num_eq(&family, &n(7)) {
                    for tag_info in info_array {
                        let mut id = tag_info.get("TagID");
                        if id.is_undef() {
                            id = s(""); // (just to be safe)
                        } else if re(r"^\d+$", "").is_match(&id) {
                            if self_.truthy() && opt(&self_, "HexTagIDs").truthy() {
                                id = pl::sprintf("0x%x", &[id]);
                            }
                        } else {
                            re(r"([^-_A-Za-z0-9])", "").subst(&mut id, true, |c| pl::sprintf("%.2x", &[n(cap(c, 1).to_bytes()[0] as i64)]).to_bytes().to_vec());
                        }
                        all_groups.set(pl::concat(&s("ID-"), &id).to_bytes(), n(1));
                    }
                } else {
                    for tag_info in info_array {
                        let grps = tag_info.get("Groups");
                        if !grps.truthy() {
                            continue;
                        }
                        let grp = grps.get(&fk);
                        if !grp.truthy() {
                            continue;
                        }
                        all_groups.set(grp.to_bytes(), n(1));
                    }
                }
            }
        }
        all_groups.delete("*"); // (not a real group)
        let mut k = keys(&all_groups);
        k.sort_by(|x, y| cmp(&lc(x), &lc(y)));
        ret_sorted(k, want)
    })
}

/// An eval site (`eval $code`) with the scalars `vars` as the lexicals in
/// scope, inside Writer.pl's file scope; `extra` declares more (arrays,
/// hashes). The variables get back the values the code left in them.
fn eval_vars(it: &mut Interp, code: &Value, vars: &mut [(&str, &mut Value)], extra: impl FnOnce(&Rc<Scope>), want: Want) -> ER<Value> {
    let scope = Scope::new(Some(writer_scope(it)));
    let cells: Vec<Cell> = vars.iter().map(|(name, v)| scope.scalar(name, (**v).clone())).collect();
    extra(&scope);
    let r = eval_in(it, code, &scope, want)?;
    for ((_, v), c) in vars.iter_mut().zip(cells) {
        **v = c.borrow().clone();
    }
    Ok(r)
}

/// Get new value(s) for tag
/// Inputs: 0) ExifTool object reference, 1) tag name (or tagInfo or nvHash ref, not public)
///         2) optional pointer to return new value hash reference (not part of public API)
/// Returns: List of new Raw values (list may be empty if tag is being deleted)
/// Notes: 1) Preferentially returns new value from Extra table if writable Extra tag exists
/// 2) Must call AFTER IsOverwriting() returns 1 to get proper value for shifted times
/// 3) Tag name is case sensitive and may be prefixed by family 0 or 1 group name
/// 4) Value may have been modified by CHECK_PROC routine after ValueConv
pub fn get_new_value(it: &mut Interp, a: Vec<Value>, want: Want) -> ER<Vec<Value>> {
    pl::local_topic(it, |it| {
        let self_ = arg(&a, 0);
        let tag = arg(&a, 1);
        let mut nv_hash = Value::Undef;
        if (tag.ref_type() == "HASH" && tag.get("IsNVH").truthy()) || tag.is_undef() {
            nv_hash = tag.clone();
        } else {
            let new_value_hash_pt = arg(&a, 2);
            if self_.get("NEW_VALUE").truthy() {
                let tag_info;
                if tag.is_ref() {
                    nv_hash = gnvh(it, vec![self_.clone(), tag.clone()])?;
                } else if {
                    tag_info = pl::pkg_hash(it, "Image::ExifTool::Extra").get(tag.to_bytes());
                    !tag_info.is_undef() && tag_info.get("Writable").truthy()
                } {
                    nv_hash = gnvh(it, vec![self_.clone(), tag_info])?;
                } else {
                    // separate group from tag name
                    let mut tag = tag.clone();
                    let mut groups: Vec<Value> = Vec::new();
                    let mut g1 = Value::Undef;
                    if re(r"(.*):", "").subst(&mut tag, false, |c| {
                        g1 = cap(c, 1);
                        Vec::new()
                    }) > 0
                    {
                        groups = split_on(":", &g1);
                    }
                    let tag_info_list = pl::call_list(it, "Image::ExifTool::TagLookup::FindTagInfo", vec![tag.clone()])?;
                    // decide which tag we want
                    'gnv_tag_info: for tag_info in tag_info_list {
                        let mut nvh = gnvh(it, vec![self_.clone(), tag_info.clone()])?;
                        if !nvh.truthy() {
                            continue;
                        }
                        // select tag in specified group(s) if necessary
                        for g in &groups {
                            if eq(g, &nvh.get("WriteGroup")) {
                                continue;
                            }
                            let grps = pl::method_list(it, &self_, "GetGroup", vec![tag_info.clone()])?;
                            if eq(&arg(&grps, 0), &nvh.get("WriteGroup")) {
                                // check family 1 group only if WriteGroup is not specific
                                if eq(g, &arg(&grps, 1)) {
                                    continue;
                                }
                            } else {
                                // otherwise check family 0 group
                                if eq(g, &arg(&grps, 0)) {
                                    continue;
                                }
                            }
                            // also check family 7
                            if let Some(c) = re(r"^ID-(.*)", "i").m(g) {
                                if pl::call(it, "Image::ExifTool::IsSameID", vec![tag_info.get("TagID"), cap(&c, 1)])?.truthy() {
                                    continue;
                                }
                            }
                            // step to next entry in list
                            nvh = nvh.get("Next");
                            if !nvh.truthy() {
                                continue 'gnv_tag_info;
                            }
                        }
                        nv_hash = nvh;
                        // give priority to the one we are creating
                        if !nv_hash.get("IsCreating").is_undef() {
                            break;
                        }
                    }
                }
            }
            // return new value hash if requested
            if new_value_hash_pt.truthy() {
                pl::set_deref(&new_value_hash_pt, nv_hash.clone())?;
            }
        }
        if !(nv_hash.truthy() && nv_hash.get("Value").truthy()) {
            return pl::ret_nothing(want); // return empty list
        }
        let mut vals = nv_hash.get("Value");
        // do inverse raw conversion if necessary
        // - must also check after doing a Shift
        if viv(&nv_hash, "TagInfo").get("RawConvInv").truthy() || nv_hash.get("Shift").truthy() {
            vals = Value::new_array(vals.items()); // modify a copy of the values
            let tag_info = nv_hash.get("TagInfo");
            let conv = tag_info.get("RawConvInv");
            let table = tag_info.get("Table");
            let mut check_proc = Value::Undef;
            if nv_hash.get("Shift").truthy() && table.truthy() {
                check_proc = table.get("CHECK_PROC");
            }
            let h = set_warning_ref(it);
            pl::local_sig_warn(it, h, |it| {
                it.set_scalar(EVAL_WARNING, Value::Undef);
                let count = vals.len();
                for i in 0..count {
                    let mut val = vals.at(i as i64);
                    // must check value now if it was shifted
                    if check_proc.truthy() {
                        let vr = pl::sref(val.clone());
                        let mut err = pl::call_code(it, &check_proc, vec![self_.clone(), tag_info.clone(), vr.clone()], Want::Scalar)?.pop().unwrap_or_default();
                        val = vr.deref();
                        vals.set_at(i, val.clone());
                        if err.truthy() || val.is_undef() {
                            if !err.truthy() {
                                err = s("Error generating raw value");
                            }
                            method_void(it, &self_, "Warn", vec![cat(&[&err, &s(" for "), &tag_info.get("Name")])])?;
                            array_assign(&vals, Vec::new());
                            break;
                        }
                        if !conv.truthy() {
                            continue;
                        }
                    } else if !conv.truthy() {
                        break;
                    }
                    // do inverse raw conversion
                    if conv.ref_type() == "CODE" {
                        val = pl::call_code(it, &conv, vec![val.clone(), self_.clone()], Want::Scalar)?.pop().unwrap_or_default();
                    } else {
                        //### eval RawConvInv ($self, $val, $tagInfo)
                        let (mut sv, mut ti, mut nvh, mut tg) = (self_.clone(), tag_info.clone(), nv_hash.clone(), arg(&a, 1));
                        let mut v = val.clone();
                        let r = eval_vars(it, &conv, &mut [("self", &mut sv), ("val", &mut v), ("tagInfo", &mut ti), ("nvHash", &mut nvh), ("tag", &mut tg)], |_| {}, Want::Scalar)?;
                        val = r;
                        let e = eval_error(it);
                        if e.truthy() {
                            it.set_scalar(EVAL_WARNING, e);
                        }
                    }
                    vals.set_at(i, val.clone());
                    let ew = it.get_scalar(EVAL_WARNING);
                    if ew.truthy() {
                        // an empty warning ("\n") ignores tag with no error
                        if !pl::str_eq(&ew, "\n") {
                            let err = cat(&[&clean_warning(it)?, &s(" in "), &tag_info.get("Name"), &s(" (RawConvInv)")]);
                            method_void(it, &self_, "Warn", vec![err])?;
                        }
                        array_assign(&vals, Vec::new());
                        break;
                    }
                }
                Ok(())
            })?;
        }
        // return our value(s)
        if want == Want::List {
            // remove duplicates if requested
            if vals.len() > 1 && pl::method(it, &self_, "Options", vec![s("NoDups")])?.truthy() {
                let mut seen = std::collections::HashSet::new();
                let items: Vec<Value> = vals.items().into_iter().filter(|v| seen.insert(v.to_bytes().to_vec())).collect();
                array_assign(&vals, items);
            }
            return Ok(vals.items());
        }
        ret(vals.at(0))
    })
}

/// Return true if we are deleting or overwriting the specified tag
/// Inputs: 0) ExifTool object ref, 1) new value hash reference
///         2) optional tag value (before RawConv) if deleting specific values
/// Returns: >0 - tag should be overwritten
///          =0 - the tag should be preserved
///          <0 - not sure, we need the old value to tell (if there is no old value
///               then the tag should be written if $$nvHash{IsCreating} is true)
/// Notes: $$nvHash{Value} is updated with the new value when shifting a value
pub fn is_overwriting(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (self_, nv_hash, mut val) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    if !nv_hash.truthy() {
        return ret(n(0));
    }
    // overwrite regardless if no DelValues specified
    if !nv_hash.get("DelValue").truthy() {
        return ret(n(1));
    }
    // never overwrite if DelValue list exists but is empty
    let shift = nv_hash.get("Shift");
    if nv_hash.get("DelValue").len() == 0 && shift.is_undef() {
        return ret(n(0));
    }
    // return "don't know" if we don't have a value to test
    if val.is_undef() {
        return ret(n(-1));
    }
    // apply raw conversion if necessary
    let tag_info = nv_hash.get("TagInfo");
    let conv = tag_info.get("RawConv");
    if conv.truthy() {
        let h = set_warning_ref(it);
        val = pl::local_sig_warn(it, h, |it| {
            it.set_scalar(EVAL_WARNING, Value::Undef);
            if conv.ref_type() == "CODE" {
                Ok(pl::call_code(it, &conv, vec![val.clone(), self_.clone()], Want::Scalar)?.pop().unwrap_or_default())
            } else {
                let (mut priority, mut tag) = (Value::Undef, tag_info.get("Name"));
                //### eval RawConv ($self, $val, $tag, $tagInfo, $priority, @grps)
                let (mut sv, mut v, mut ti, mut nvh) = (self_.clone(), val.clone(), tag_info.clone(), nv_hash.clone());
                let r = eval_vars(
                    it,
                    &conv,
                    &mut [("self", &mut sv), ("val", &mut v), ("tag", &mut tag), ("tagInfo", &mut ti), ("priority", &mut priority), ("nvHash", &mut nvh)],
                    |sc| {
                        sc.array("grps", Vec::new());
                    },
                    Want::Scalar,
                )?;
                let e = eval_error(it);
                if e.truthy() {
                    it.set_scalar(EVAL_WARNING, e);
                }
                Ok(r)
            }
        })?;
        if val.is_undef() {
            return ret(n(-1));
        }
    }
    // do not overwrite if only creating
    if nv_hash.get("CreateOnly").truthy() {
        return ret(n(0));
    }
    // apply time/number shift if necessary
    if !shift.is_undef() {
        let mut shift_type = tag_info.get("Shift");
        if !(shift_type.truthy() && pl::str_eq(&shift_type, "Time")) {
            if !pl::call(it, "Image::ExifTool::IsFloat", vec![val.clone()])?.truthy() {
                // do the ValueConv to try to get a number
                let conv = tag_info.get("ValueConv");
                if !conv.is_undef() {
                    let h = set_warning_ref(it);
                    let bad = pl::local_sig_warn(it, h, |it| {
                        it.set_scalar(EVAL_WARNING, Value::Undef);
                        if conv.ref_type() == "CODE" {
                            val = pl::call_code(it, &conv, vec![val.clone(), self_.clone()], Want::Scalar)?.pop().unwrap_or_default();
                        } else if !conv.is_ref() {
                            //### eval ValueConv ($val, $self)
                            let (mut sv, mut v, mut ti, mut nvh) = (self_.clone(), val.clone(), tag_info.clone(), nv_hash.clone());
                            let r = eval_vars(it, &conv, &mut [("val", &mut v), ("self", &mut sv), ("tagInfo", &mut ti), ("nvHash", &mut nvh)], |_| {}, Want::Scalar)?;
                            val = r;
                            let e = eval_error(it);
                            if e.truthy() {
                                it.set_scalar(EVAL_WARNING, e);
                            }
                        }
                        if it.get_scalar(EVAL_WARNING).truthy() {
                            let cw = clean_warning(it)?;
                            method_void(it, &self_, "Warn", vec![cat(&[&s("ValueConv "), &tag_info.get("Name"), &s(": "), &cw])])?;
                            return Ok(true);
                        }
                        Ok(false)
                    })?;
                    if bad {
                        return ret(n(0));
                    }
                }
                if !(!val.is_undef() && pl::call(it, "Image::ExifTool::IsFloat", vec![val.clone()])?.truthy()) {
                    method_void(it, &self_, "Warn", vec![cat(&[&s("Can't shift "), &tag_info.get("Name"), &s(" (not a number)")])])?;
                    return ret(n(0));
                }
            }
            shift_type = s("Number"); // allow any number to be shifted
        }
        crate::runtime::require(it, "Image/ExifTool/Shift.pl")?;
        let err = pl::method(it, &self_, "ApplyShift", vec![shift_type, shift, val.clone(), nv_hash.clone()])?;
        if err.truthy() {
            method_void(it, &self_, "Warn", vec![cat(&[&err, &s(" when shifting "), &tag_info.get("Name")])])?;
            return ret(n(0));
        }
        // ensure that the shifted value is valid and reformat if necessary
        let check_val = get_new_value(it, vec![self_.clone(), nv_hash.clone()], Want::Scalar)?.pop().unwrap_or_default();
        if check_val.is_undef() {
            return ret(n(0));
        }
        // don't bother overwriting if value is the same
        if eq(&val, &nv_hash.get("Value").at(0)) {
            return ret(n(0));
        }
        return ret(n(1));
    }
    // return 1 if value matches a DelValue
    for del_val in nv_hash.get("DelValue").items() {
        if eq(&val, &del_val) {
            return ret(n(1));
        }
    }
    ret(n(0))
}

/// `$self->IsOverwriting($nvHash[, $val])`
fn overwriting(it: &mut Interp, self_: &Value, nv_hash: &Value, val: Option<Value>) -> ER<Value> {
    let mut args = vec![self_.clone(), nv_hash.clone()];
    if let Some(v) = val {
        args.push(v);
    }
    Ok(is_overwriting(it, args, Want::Scalar)?.pop().unwrap_or_default())
}

/// `$self->VPrint(2, $msg)`
fn vprint2(it: &mut Interp, self_: &Value, msg: Value) -> ER<()> {
    method_void(it, self_, "VPrint", vec![n(2), msg])
}

/// Apply inverse conversions
/// Inputs: 0) ExifTool ref, 1) value, 2) tagInfo (or Struct item) ref,
///         3) tag name, 4) group 1 name, 5) conversion type (or undef),
///         6) [optional] want group ("" for structure field)
/// Returns: 0) converted value, 1) error string (or undef on success)
/// Notes:
/// - uses ExifTool "ConvType" member when conversion type is undef
/// - conversion types other than 'ValueConv' and 'PrintConv' are treated as 'Raw'
pub fn conv_inv(it: &mut Interp, a: Vec<Value>, want: Want) -> ER<Vec<Value>> {
    let (self_, mut val, tag_info, tag, wgrp1, mut conv_type, want_group) = (arg(&a, 0), arg(&a, 1), arg(&a, 2), arg(&a, 3), arg(&a, 4), arg(&a, 5), arg(&a, 6));
    let mut err = Value::Undef;
    let mut type_ = Value::Undef;

    if !conv_type.truthy() {
        conv_type = self_.get("ConvType");
        if !conv_type.truthy() {
            conv_type = s("PrintConv");
        }
    }
    let wt = cat(&[&wgrp1, &s(":"), &tag]); // "$wgrp1:$tag"

    'conv: loop {
        if type_.is_undef() {
            // split value into list if necessary
            if tag_info.get("List").truthy() {
                let mut list_split = tag_info.get("AutoSplit");
                if !list_split.truthy() {
                    list_split = opt(&self_, "ListSplit");
                }
                if !list_split.is_undef() && !tag_info.get("Struct").truthy() && (want_group.truthy() || want_group.is_undef()) {
                    if pl::str_eq(&list_split, "1") && tag_info.get("AutoSplit").truthy() {
                        list_split = s(r",?\s+");
                    }
                    let split_val = pl::split(&pl::re_dyn(&list_split.to_bytes(), "")?, &val, -1);
                    val = if split_val.len() > 1 {
                        Value::new_array(split_val)
                    } else if !split_val.is_empty() {
                        split_val[0].clone()
                    } else {
                        s("")
                    };
                }
            }
            type_ = conv_type.clone();
        } else if pl::str_eq(&type_, "PrintConv") {
            type_ = s("ValueConv");
        } else {
            // split raw value if necessary
            if tag_info.get("RawJoin").truthy() && tag_info.get("List").truthy() && !val.is_ref() {
                let split_val = pl::split_ws(&val);
                if split_val.len() > 1 {
                    val = Value::new_array(split_val);
                }
            }
            // finally, do our value check
            let mut err2 = Value::Undef;
            let write_check = tag_info.get("WriteCheck");
            if write_check.truthy() {
                //### eval WriteCheck ($self, $tagInfo, $val)
                let (mut sv, mut ti, mut tg, mut wg1, mut ct, mut wg, mut ty) = (self_.clone(), tag_info.clone(), tag.clone(), wgrp1.clone(), conv_type.clone(), want_group.clone(), type_.clone());
                err2 = eval_vars(
                    it,
                    &write_check,
                    &mut [("self", &mut sv), ("tagInfo", &mut ti), ("val", &mut val), ("tag", &mut tg), ("wgrp1", &mut wg1), ("convType", &mut ct), ("wantGroup", &mut wg), ("type", &mut ty)],
                    |_| {},
                    Want::Scalar,
                )?;
                let e = eval_error(it);
                if e.truthy() {
                    pl::warn(it, &e.to_text())?;
                    err2 = s("Error evaluating WriteCheck");
                }
            }
            if err2.is_undef() {
                let table = tag_info.get("Table");
                if table.truthy() && table.get("CHECK_PROC").truthy() && !tag_info.get("RawConvInv").truthy() {
                    let check_proc = table.get("CHECK_PROC");
                    if val.ref_type() == "ARRAY" {
                        // loop through array values
                        for i in 0..val.len() {
                            let vr = pl::sref(val.at(i as i64));
                            err2 = pl::call_code(it, &check_proc, vec![self_.clone(), tag_info.clone(), vr.clone(), conv_type.clone()], Want::Scalar)?.pop().unwrap_or_default();
                            val.set_at(i, vr.deref());
                            if err2.truthy() {
                                break;
                            }
                        }
                    } else {
                        let vr = pl::sref(val.clone());
                        err2 = pl::call_code(it, &check_proc, vec![self_.clone(), tag_info.clone(), vr.clone(), conv_type.clone()], Want::Scalar)?.pop().unwrap_or_default();
                        val = vr.deref();
                    }
                }
            }
            if !err2.is_undef() {
                if err2.truthy() {
                    err = cat(&[&err2, &s(" for "), &wt]);
                    vprint2(it, &self_, cat(&[&err, &s("\n")]))?;
                    val = Value::Undef; // value was invalid
                } else {
                    err = err2; // empty error (quietly don't write tag)
                }
            }
            break;
        }
        let tk = type_.to_bytes();
        let tik = cat(&[&type_, &s("Inv")]).to_bytes();
        let mut conv = tag_info.get(&tk);
        let mut conv_inv_v = tag_info.get(&tik);
        // nothing to do at this level if no conversion defined
        if conv.is_undef() && conv_inv_v.is_undef() {
            continue;
        }

        let mut val_list: Vec<Value> = Vec::new();
        let mut index = 0usize;
        let (mut conv_list, mut conv_inv_list) = (Value::Undef, Value::Undef);
        if val.ref_type() == "ARRAY" {
            // handle ValueConv of ListSplit and AutoSplit values
            val_list = val.items();
            val = val_list.first().cloned().unwrap_or_default();
        } else if conv.ref_type() == "ARRAY" || conv_inv_v.ref_type() == "ARRAY" {
            // handle conversion lists
            let sep = my_hash(it, "listSep").get(&tk);
            val_list = pl::split(&pl::re_dyn(&sep.to_bytes(), "")?, &val, 0);
            val = val_list.first().cloned().unwrap_or_default();
            if conv.ref_type() == "ARRAY" {
                conv_list = conv.clone();
                conv = conv.at(0);
            }
            if conv_inv_v.ref_type() == "ARRAY" {
                conv_inv_list = conv_inv_v.clone();
                conv_inv_v = conv_inv_v.at(0);
            }
        }
        // loop through multiple values if necessary
        loop {
            if conv_inv_v.truthy() {
                // capture eval warnings too
                let h = set_warning_ref(it);
                let stop = pl::local_sig_warn(it, h, |it| {
                    it.set_scalar(EVAL_WARNING, Value::Undef);
                    if conv_inv_v.ref_type() == "CODE" {
                        val = pl::call_code(it, &conv_inv_v, vec![val.clone(), self_.clone()], Want::Scalar)?.pop().unwrap_or_default();
                    } else {
                        //### eval PrintConvInv/ValueConvInv ($val, $self, $wantGroup)
                        let (mut v, mut sv, mut wg, mut ti, mut tg, mut wg1, mut ct, mut ty) = (val.clone(), self_.clone(), want_group.clone(), tag_info.clone(), tag.clone(), wgrp1.clone(), conv_type.clone(), type_.clone());
                        val = eval_vars(
                            it,
                            &conv_inv_v,
                            &mut [("val", &mut v), ("self", &mut sv), ("wantGroup", &mut wg), ("tagInfo", &mut ti), ("tag", &mut tg), ("wgrp1", &mut wg1), ("convType", &mut ct), ("type", &mut ty)],
                            |_| {},
                            Want::Scalar,
                        )?;
                        let e = eval_error(it);
                        if e.truthy() {
                            it.set_scalar(EVAL_WARNING, e);
                        }
                    }
                    let ew = it.get_scalar(EVAL_WARNING);
                    if ew.truthy() {
                        // an empty warning ("\n") ignores tag with no error
                        if pl::str_eq(&ew, "\n") {
                            if err.is_undef() {
                                err = s("");
                            }
                        } else {
                            err = cat(&[&clean_warning(it)?, &s(" in "), &wt, &s(" ("), &type_, &s("Inv)")]);
                            vprint2(it, &self_, cat(&[&err, &s("\n")]))?;
                        }
                        val = Value::Undef;
                        return Ok(true);
                    } else if val.is_undef() {
                        err = cat(&[&s("Error converting value for "), &wt, &s(" ("), &type_, &s("Inv)")]);
                        vprint2(it, &self_, cat(&[&err, &s("\n")]))?;
                        return Ok(true);
                    }
                    Ok(false)
                })?;
                if stop {
                    break 'conv;
                }
            } else if conv.truthy() {
                if conv.ref_type() == "HASH" && (!tag_info.exists(&tik) || conv_inv_list.truthy()) {
                    let mut multi = Value::Undef;
                    // insert alternate language print conversions if required
                    let cur_lang = self_.get("CUR_LANG");
                    let mut lc_;
                    if cur_lang.truthy() && pl::str_eq(&type_, "PrintConv") && {
                        lc_ = viv(&self_, "CUR_LANG").get(tag.to_bytes());
                        lc_.ref_type() == "HASH"
                    } && {
                        lc_ = lc_.get("PrintConv");
                        lc_.truthy()
                    } {
                        let new_conv = Value::new_hash();
                        for k in keys(&conv) {
                            let v = conv.get(k.to_bytes());
                            if lc_.get(v.to_bytes()).is_undef() {
                                new_conv.set(k.to_bytes(), v);
                                continue;
                            }
                            let d = pl::method(it, &self_, "Decode", vec![lc_.get(v.to_bytes()), s("UTF8")])?;
                            new_conv.set(k.to_bytes(), d);
                        }
                        if conv.get("BITMASK").truthy() {
                            let bm = conv.get("BITMASK");
                            for k in keys(&bm) {
                                let v = bm.get(k.to_bytes());
                                if lc_.get(v.to_bytes()).is_undef() {
                                    viv(&new_conv, "BITMASK").set(k.to_bytes(), v);
                                    continue;
                                }
                                let d = pl::method(it, &self_, "Decode", vec![lc_.get(v.to_bytes()), s("UTF8")])?;
                                viv(&new_conv, "BITMASK").set(k.to_bytes(), d);
                            }
                        }
                        conv = new_conv;
                    }
                    it.set_scalar(EVAL_WARNING, Value::Undef);
                    if conv.get("BITMASK").truthy() {
                        let lookup_bits = conv.get("BITMASK");
                        let (wbits, tbits) = (tag_info.get("BitsPerWord"), tag_info.get("BitsTotal"));
                        let r = pl::call_list(it, "Image::ExifTool::EncodeBits", vec![val.clone(), lookup_bits.clone(), wbits, tbits])?;
                        let (val2, err2) = (arg(&r, 0), arg(&r, 1));
                        if err2.truthy() {
                            // ok, try matching a straight value
                            let r = reverse_lookup(it, vec![val.clone(), conv.clone()], Want::List)?;
                            val = arg(&r, 0);
                            multi = arg(&r, 1);
                            if val.is_undef() {
                                err = cat(&[&s("Can't encode "), &wt, &s(" ("), &err2, &s(")")]);
                                vprint2(it, &self_, cat(&[&err, &s("\n")]))?;
                                break 'conv;
                            }
                        } else if !val2.is_undef() {
                            val = val2;
                        } else {
                            conv.delete("BITMASK");
                            let r = reverse_lookup(it, vec![val.clone(), conv.clone()], Want::List)?;
                            val = arg(&r, 0);
                            multi = arg(&r, 1);
                            conv.set("BITMASK", lookup_bits);
                        }
                    } else {
                        let r = reverse_lookup(it, vec![val.clone(), conv.clone()], Want::List)?;
                        val = arg(&r, 0);
                        multi = arg(&r, 1);
                    }
                    if val.is_undef() {
                        let prob = if it.get_scalar(EVAL_WARNING).truthy() {
                            pl::lcfirst(&clean_warning(it)?)
                        } else {
                            pl::concat(&s(if multi.truthy() { "matches more than one " } else { "not in " }), &type_)
                        };
                        err = cat(&[&s("Can't convert "), &wt, &s(" ("), &prob, &s(")")]);
                        vprint2(it, &self_, cat(&[&err, &s("\n")]))?;
                        break 'conv;
                    } else if it.get_scalar(EVAL_WARNING).truthy() {
                        let cw = clean_warning(it)?;
                        vprint2(it, &self_, cat(&[&cw, &s(" for "), &wt, &s("\n")]))?;
                    }
                } else if !tag_info.get("WriteAlso").truthy() {
                    err = cat(&[&s("Can't convert value for "), &wt, &s(" (no "), &type_, &s("Inv)")]);
                    vprint2(it, &self_, cat(&[&err, &s("\n")]))?;
                    val = Value::Undef;
                    break 'conv;
                }
            }
            if val_list.is_empty() {
                break;
            }
            val_list[index] = val.clone();
            index += 1;
            if index >= val_list.len() {
                // leave AutoSplit lists in ARRAY form, or join conversion lists
                val = if tag_info.get("List").truthy() { Value::new_array(val_list.clone()) } else { pl::join(" ", &val_list) };
                break;
            }
            if conv_list.truthy() {
                conv = conv_list.at(index as i64);
            }
            if conv_inv_list.truthy() {
                conv_inv_v = conv_inv_list.at(index as i64);
            }
            val = val_list[index].clone();
        }
    } // end ValueConv/PrintConv loop

    ret_list(vec![val, err], want)
}

/// initialize ADD_DIRS and EDIT_DIRS hashes for all directories that need
/// to be created or will have tags changed in them
/// Inputs: 0) ExifTool object reference, 1) file type string (or map hash ref)
///         2) preferred family 0 group for creating tags, 3) alternate preferred group
/// Notes:
/// - the ADD_DIRS and EDIT_DIRS keys are the directory names, and the values
///   are the names of the parent directories (undefined for a top-level directory)
/// - also initializes FORCE_WRITE lookup
pub fn init_write_dirs(it: &mut Interp, a: Vec<Value>, want: Want) -> ER<Vec<Value>> {
    let (self_, file_type, preferred_group, alt_group) = (arg(&a, 0), arg(&a, 1), arg(&a, 2), arg(&a, 3));
    let edit_dirs = Value::new_hash();
    self_.set("EDIT_DIRS", edit_dirs.clone());
    let add_dirs = Value::new_hash();
    self_.set("ADD_DIRS", add_dirs.clone());
    let mut file_dirs = my_hash(it, "dirMap").get(file_type.to_bytes());
    if !file_dirs.truthy() {
        if file_type.ref_type() != "HASH" {
            return pl::ret_nothing(want);
        }
        file_dirs = file_type.clone();
    }
    let tag_info_list = get_new_tag_info_list(it, vec![self_.clone()], Want::List)?;

    // save the preferred group
    self_.set("PreferredGroup", preferred_group.clone());

    for tag_info in tag_info_list {
        // cycle through all hashes in linked list
        let mut nv_hash = gnvh(it, vec![self_.clone(), tag_info.clone()])?;
        while nv_hash.truthy() {
            // are we creating this tag? (otherwise just deleting or editing it)
            let mut is_creating = nv_hash.get("IsCreating");
            if preferred_group.truthy() {
                let g0 = pl::method(it, &self_, "GetGroup", vec![tag_info.clone(), n(0)])?;
                if is_creating.truthy() {
                    // if another group is taking priority, only create
                    // directory if specifically adding tags to this group
                    // or if this tag isn't being added to the priority group
                    if !eq(&preferred_group, &g0) && viv(&nv_hash, "CreateGroups").get(preferred_group.to_bytes()).truthy() && (!alt_group.truthy() || !eq(&alt_group, &g0)) {
                        is_creating = n(0);
                    }
                } else {
                    // create this directory if any tag is preferred and has a value
                    // (unless group creation is disabled via the WriteMode option)
                    if nv_hash.get("Value").truthy() && eq(&preferred_group, &g0) && !nv_hash.get("EditOnly").truthy() && re(r"g", "").is_match(&opt(&self_, "WriteMode")) {
                        is_creating = n(1);
                    }
                }
            }
            // tag belongs to directory specified by WriteGroup, or by
            // the Group0 name if WriteGroup not defined
            let mut dir_name = nv_hash.get("WriteGroup");
            // remove MIE copy number(s) if they exist
            if let Some(c) = re(r"^MIE\d*(-[a-z]+)?\d*$", "i").m(&dir_name) {
                let g = cap(&c, 1);
                dir_name = pl::concat(&s("MIE"), &(if g.truthy() { g } else { s("") }));
            }
            let mut dir_names: std::collections::VecDeque<Value> = Default::default();
            // allow a group name of '*' to force writing EXIF/IPTC/XMP/PNG (ForceWrite tag)
            if pl::str_eq(&dir_name, "*") && nv_hash.get("Value").truthy() {
                let val = nv_hash.get("Value").at(0);
                if val.truthy() {
                    for x in ["EXIF", "IPTC", "XMP", "PNG", "FixBase"] {
                        if !pl::re_dyn(format!(r"\b({x}|All)\b").as_bytes(), "i")?.is_match(&val) {
                            continue;
                        }
                        dir_names.push_back(s(x));
                        if x == "FixBase" {
                            dir_names.push_back(s("EXIF"));
                        }
                        viv(&self_, "FORCE_WRITE").set(x, n(1));
                    }
                }
                dir_name = dir_names.pop_front().unwrap_or_default();
            } else if pl::str_eq(&dir_name, "QuickTime") {
                // write to specific QuickTime group
                dir_name = pl::method(it, &self_, "GetGroup", vec![tag_info.clone(), n(1)])?;
            }
            while dir_name.truthy() {
                let mut parent = file_dirs.get(dir_name.to_bytes());
                if parent.is_ref() {
                    let mut items = parent.items();
                    items.reverse();
                    dir_names.extend(items);
                    parent = dir_names.pop_back().unwrap_or_default();
                }
                edit_dirs.set(dir_name.to_bytes(), parent.clone());
                if is_creating.truthy() && !pl::num_eq(&is_creating, &n(2)) {
                    add_dirs.set(dir_name.to_bytes(), parent.clone());
                }
                dir_name = if parent.truthy() { parent } else { dir_names.pop_front().unwrap_or_default() };
            }
            nv_hash = nv_hash.get("Next");
        }
    }
    if !hash_rv(&self_.get("DEL_GROUP"))?.is_empty() {
        // add delete groups to list of edited groups
        let translate_write_group = my_hash(it, "translateWriteGroup");
        for k in keys(&self_.get("DEL_GROUP")) {
            if re(r"^-", "").is_match(&k) {
                continue; // ignore excluded groups
            }
            let mut dir_name = k.clone();
            // translate necessary group 0 names
            let tr = translate_write_group.get(dir_name.to_bytes());
            if tr.truthy() {
                dir_name = tr;
            }
            // convert XMP group 1 names
            if re(r"^XMP-", "").is_match(&dir_name) {
                dir_name = s("XMP");
            }
            let mut dir_names: std::collections::VecDeque<Value> = Default::default();
            while dir_name.truthy() {
                let mut parent = file_dirs.get(dir_name.to_bytes());
                if parent.is_ref() {
                    let mut items = parent.items();
                    items.reverse();
                    dir_names.extend(items);
                    parent = dir_names.pop_back().unwrap_or_default();
                }
                edit_dirs.set(dir_name.to_bytes(), parent.clone());
                dir_name = if parent.truthy() { parent } else { dir_names.pop_front().unwrap_or_default() };
            }
        }
    }
    // special case to edit JFIF to get resolutions if editing EXIF information
    if edit_dirs.get("IFD0").truthy() && file_dirs.get("JFIF").truthy() {
        edit_dirs.set("JFIF", s("IFD1"));
        edit_dirs.set("APP0", Value::Undef);
    }

    if opt(&self_, "Verbose").truthy() {
        let out = opt(&self_, "TextOut");
        print_out(it, &out, s("  Editing tags in: "))?;
        for k in sort_str(keys(&edit_dirs)) {
            print_out(it, &out, pl::concat(&k, &s(" ")))?;
        }
        print_out(it, &out, s("\n"))?;
        if !pl::gt(&opt(&self_, "Verbose"), &n(1)) {
            return pl::ret_nothing(want);
        }
        print_out(it, &out, s("  Creating tags in: "))?;
        for k in sort_str(keys(&add_dirs)) {
            print_out(it, &out, pl::concat(&k, &s(" ")))?;
        }
        print_out(it, &out, s("\n"))?;
    }
    pl::ret_undef()
}

/// Write an image directory
/// Inputs: 0) ExifTool object reference, 1) source directory information reference
///         2) tag table reference, 3) optional reference to writing procedure
/// Returns: New directory data or undefined on error (or empty string to delete directory)
pub fn write_directory(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (self_, dir_info, tag_table_ptr, mut write_proc) = (arg(&a, 0), arg(&a, 1), arg(&a, 2), arg(&a, 3));
    let mut out = Value::Undef;
    let mut del_flag;

    if !tag_table_ptr.truthy() {
        return pl::ret_undef();
    }
    if opt(&self_, "Verbose").truthy() {
        out = opt(&self_, "TextOut");
    }
    // set directory name from default group0 name if not done already
    let mut dir_name = dir_info.get("DirName");
    let mut parent = dir_info.get("Parent");
    if !parent.truthy() {
        parent = s("");
    }
    let data_pt = dir_info.get("DataPt");
    let grp0 = viv(&tag_table_ptr, "GROUPS").get("0");
    if !dir_name.truthy() {
        dir_name = grp0.clone();
        dir_info.set("DirName", dir_name.clone());
    }
    if !hash_rv(&self_.get("DEL_GROUP"))?.is_empty() {
        let del_group = self_.get("DEL_GROUP");
        // delete entire directory if specified
        let mut grp1 = dir_name.clone();
        del_flag = del_group.get(grp0.to_bytes());
        if !del_flag.truthy() {
            del_flag = del_group.get(grp1.to_bytes());
        }
        if my_hash(it, "permanentDir").get(grp0.to_bytes()).truthy() && !(dir_info.get("TagInfo").truthy() && viv(&dir_info, "TagInfo").get("Deletable").truthy()) {
            del_flag = Value::Undef;
        }
        if del_flag.truthy() {
            let block_exif_types = my_hash(it, "blockExifTypes");
            let raw_type = is_raw_type(it, vec![self_.clone()], Want::Scalar)?.pop().unwrap_or_default();
            // ($1 of whichever match succeeded)
            let m1 = match re(r"^(MakerNotes)$", "").m(&grp0) {
                Some(c) => Some(cap(&c, 1)),
                None => re(r"^(IFD0|ExifIFD|MakerNotes)$", "").m(&grp1).map(|c| cap(&c, 1)),
            };
            let ti = dir_info.get("TagInfo");
            if dir_info.get("Permanent").truthy() {
                method_void(it, &self_, "Warn", vec![cat(&[&s("Not deleting permanent "), &dir_name, &s(" directory")])])?;
                grp1 = Value::Undef;
            } else if m1.is_some()
                && raw_type.truthy()
                // allow non-permanent MakerNote directories to be deleted (ie. NikonCapture)
                && (!ti.truthy() || ti.get("Permanent").is_undef() || ti.get("Permanent").truthy())
                // allow MakerNotes to be deleted from ExifIFD of CR3 file
                && !(pl::num_eq(&raw_type, &n(2)) && pl::str_eq(&parent, "ExifIFD"))
            {
                let m1 = m1.unwrap_or_default();
                method_void(it, &self_, "Warn", vec![cat(&[&s("Can't delete "), &m1, &s(" from "), &self_.get("FileType")]), n(1)])?;
                grp1 = Value::Undef;
            } else if !block_exif_types.get(self_.get("FILE_TYPE").to_bytes()).truthy() {
                // restrict delete logic to prevent entire tiff image from being killed
                // (don't allow IFD0 to be deleted, and delete only ExifIFD if EXIF specified)
                let ft = self_.get("FILE_TYPE");
                if pl::str_eq(&ft, "PSD") {
                    // don't delete Photoshop directories from PSD image
                    if pl::str_eq(&grp0, "Photoshop") {
                        grp1 = Value::Undef;
                    }
                } else if re(r"^(EPS|PS)$", "").is_match(&ft) {
                    // allow anything to be deleted from PostScript files
                } else if pl::str_eq(&grp1, "IFD0") {
                    let mut type_ = self_.get("TIFF_TYPE");
                    if !type_.truthy() {
                        type_ = ft.clone();
                    }
                    if del_group.get("IFD0").truthy() {
                        method_void(it, &self_, "Warn", vec![cat(&[&s("Can't delete IFD0 from "), &type_]), n(1)])?;
                    }
                    grp1 = Value::Undef;
                } else if pl::str_eq(&grp0, "EXIF") && del_group.get(grp0.to_bytes()).truthy() {
                    if !(del_group.get(grp1.to_bytes()).truthy() || pl::str_eq(&grp1, "ExifIFD")) {
                        grp1 = Value::Undef;
                    }
                }
            }
            if grp1.truthy() {
                if data_pt.truthy() || dir_info.get("RAF").truthy() {
                    pre_inc(&self_, "CHANGED");
                    if out.truthy() {
                        print_out(it, &out, cat(&[&s("  Deleting "), &grp1, &s("\n")]))?;
                    }
                    if pl::str_eq(&grp1, "ICC_Profile") {
                        method_void(it, &self_, "Warn", vec![s("ICC_Profile deleted. Image colors may be affected")])?;
                    }
                    // can no longer validate TIFF_END if deleting an entire IFD
                    if re(r"IFD", "").is_match(&dir_name) {
                        self_.delete("TIFF_END");
                    }
                }
                // don't add back into the wrong location
                let add_dirs = viv(&self_, "ADD_DIRS");
                let mut right = add_dirs.get(grp1.to_bytes());
                // (take care because EXIF directory name may be either EXIF or IFD0,
                //  but IFD0 will be the one that appears in the directory map)
                if !right.truthy() && pl::str_eq(&grp1, "EXIF") {
                    right = add_dirs.get("IFD0");
                }
                if pl::num_eq(&del_flag, &n(2)) && right.truthy() {
                    // also check grandparent because some routines create 2 levels in 1
                    let mut right2 = add_dirs.get(right.to_bytes());
                    if !right2.truthy() {
                        right2 = s("");
                    }
                    if !parent.truthy() || eq(&parent, &right) || eq(&parent, &right2) {
                        // prevent duplicate directories from being recreated at the same path
                        let mut parts = viv_array(&self_, "PATH").items();
                        parts.push(dir_name.clone());
                        let path = pl::join("-", &parts);
                        if !self_.get("Recreated").truthy() {
                            self_.set("Recreated", Value::new_hash());
                        }
                        let recreated = self_.get("Recreated");
                        if recreated.get(path.to_bytes()).truthy() {
                            let p = if parent.truthy() { pl::concat(&s(" in "), &parent) } else { s("") };
                            method_void(it, &self_, "Warn", vec![cat(&[&s("Not recreating duplicate "), &grp1, &p]), n(1)])?;
                            return ret(s(""));
                        }
                        recreated.set(path.to_bytes(), n(1));
                        // empty the directory
                        let data = s("");
                        dir_info.set("DataPt", pl::sref(data));
                        dir_info.set("DataLen", n(0));
                        dir_info.set("DirStart", n(0));
                        dir_info.set("DirLen", n(0));
                        dir_info.delete("RAF");
                        dir_info.delete("Base");
                        dir_info.delete("DataPos");
                    } else {
                        method_void(it, &self_, "Warn", vec![cat(&[&s("Not recreating "), &grp1, &s(" in "), &parent, &s(" (should be in "), &right, &s(")")]), n(1)])?;
                        return ret(s(""));
                    }
                } else if !dir_info.get("NoDelete").truthy() {
                    return ret(s(""));
                }
            }
        }
    }
    // use default proc from tag table if no proc specified
    if !write_proc.truthy() {
        write_proc = tag_table_ptr.get("WRITE_PROC");
        if !write_proc.truthy() {
            return pl::ret_undef();
        }
    }

    // are we rewriting a pre-existing directory?
    let is_rewriting = dir_info.get("DirLen").truthy() || (!data_pt.is_undef() && length_of(&data_pt.deref()) > 0) || dir_info.get("RAF").truthy();

    // copy or delete new directory as a block if specified
    let mut block_name = dir_name.clone();
    if pl::str_eq(&block_name, "IFD0") {
        block_name = s("EXIF");
    }
    let mut tag_info = pl::pkg_hash(it, "Image::ExifTool::Extra").get(block_name.to_bytes());
    if !tag_info.truthy() {
        tag_info = dir_info.get("TagInfo");
    }
    let dp = data_pt.clone();
    loop {
        if !tag_info.truthy() {
            break;
        }
        let nv_hash = viv(&self_, "NEW_VALUE").get(tag_info.to_bytes());
        if !nv_hash.truthy() || !overwriting(it, &self_, &nv_hash, None)?.truthy() || (nv_hash.get("CreateOnly").truthy() && is_rewriting) {
            break;
        }
        // protect against writing EXIF to wrong file types, etc
        if pl::str_eq(&block_name, "EXIF") {
            if !my_hash(it, "blockExifTypes").get(self_.get("FILE_TYPE").to_bytes()).truthy() {
                method_void(it, &self_, "Warn", vec![cat(&[&s("Can't write EXIF as a block to "), &self_.get("FILE_TYPE"), &s(" file")])])?;
                break;
            }
            // this can happen if we call WriteDirectory for an EXIF directory without going
            // through WriteTIFF as the WriteProc (which happens if conditionally replacing
            // the EXIF block and the condition fails), but we never want to do a block write
            // in this case because the EXIF block would end up with two TIFF headers
            let wt = Value::Code(it.sub_ref("Image::ExifTool::WriteTIFF"));
            if !eq(&write_proc, &wt) {
                break;
            }
        }
        let dv = if dp.truthy() { dp.deref() } else { s("") };
        if !overwriting(it, &self_, &nv_hash, Some(dv))?.truthy() {
            break;
        }
        let mut verb = "Writing";
        let mut new_val = get_new_value(it, vec![self_.clone(), nv_hash.clone()], Want::Scalar)?.pop().unwrap_or_default();
        if !new_val.is_undef() && length_of(&new_val) > 0 {
            // hack to add back TIFF header when writing MakerNoteCanon to CMT3 in CR3 images
            if pl::str_eq(&tag_info.get("Name"), "MakerNoteCanon") {
                crate::runtime::require(it, "Image::ExifTool::Canon")?;
                if eq(&tag_info, &pl::pkg_hash(it, "Image::ExifTool::Canon::uuid").get("CMT3")) {
                    let hdr = if pl::str_eq(&pl::substr(&new_val, 0, Some(1)), "\0") {
                        pl::concat(&Value::bytes(b"MM\0\x2a"), &pl::pack("N", &[n(8)])?)
                    } else {
                        pl::concat(&Value::bytes(b"II\x2a\0"), &pl::pack("V", &[n(8)])?)
                    };
                    new_val = pl::concat(&hdr, &new_val);
                }
            }
        } else {
            if !(dp.truthy() || dir_info.get("RAF").truthy()) {
                return ret(s("")); // nothing to do if block never existed
            }
            // don't allow MakerNotes to be removed from RAW files
            let raw_type = is_raw_type(it, vec![self_.clone()], Want::Scalar)?.pop().unwrap_or_default();
            if pl::str_eq(&block_name, "MakerNotes")
                && raw_type.truthy()
                // but allow MakerNotes to be deleted from ExifIFD of CR3 image (shouldn't be there)
                && !(pl::num_eq(&raw_type, &n(2)) && pl::str_eq(&parent, "ExifIFD"))
            {
                method_void(it, &self_, "Warn", vec![cat(&[&s("Can't delete MakerNotes from "), &self_.get("FileType")]), n(1)])?;
                return pl::ret_undef();
            }
            verb = "Deleting";
            new_val = s("");
        }
        dir_info.set("BlockWrite", n(1)); // set flag indicating we did a block write
        if out.truthy() {
            print_out(it, &out, cat(&[&s(&format!("  {verb} ")), &block_name, &s(" as a block\n")]))?;
        }
        pre_inc(&self_, "CHANGED");
        return ret(new_val);
    }
    // guard against writing the same directory twice
    if !dp.is_undef() && !dir_info.get("DirStart").is_undef() && !dir_info.get("DataPos").is_undef() && !dir_info.get("NoRefTest").truthy() {
        let base = dir_info.get("Base");
        let addr = pl::add(&pl::add(&pl::add(&dir_info.get("DirStart"), &dir_info.get("DataPos")), &(if base.truthy() { base } else { n(0) })), &self_.get("BASE"));
        let processed = viv(&self_, "PROCESSED");
        let pa = processed.get(addr.to_bytes());
        // (Phase One P25 IIQ files have ICC_Profile duplicated in IFD0 and IFD1)
        if pa.truthy() && (!pl::str_eq(&dir_name, "ICC_Profile") || !pl::str_eq(&self_.get("TIFF_TYPE"), "IIQ")) {
            let dir_len = dir_info.get("DirLen");
            if !dir_len.is_undef() && !dir_len.truthy() && !eq(&dir_name, &pa) {
                // it is hypothetically possible to have 2 different directories
                // with the same address if one has a length of zero
            } else if pl::method(it, &self_, "Error", vec![cat(&[&dir_name, &s(" pointer references previous "), &pa, &s(" directory")]), n(2)])?.truthy() {
                return pl::ret_undef();
            } else {
                method_void(it, &self_, "Warn", vec![cat(&[&s("Deleting duplicate "), &dir_name, &s(" directory")])])?;
                if out.truthy() {
                    print_out(it, &out, cat(&[&s("  Deleting "), &dir_name, &s("\n")]))?;
                }
                // delete the duplicate directory (don't recreate it when writing new
                // tags to prevent propagating a duplicate IFD in cases like when the
                // same ExifIFD exists in both IFD0 and IFD1)
                return ret(s(""));
            }
        } else {
            processed.set(addr.to_bytes(), dir_name.clone());
        }
    }
    let old_dir = self_.get("DIR_NAME");
    let save = (self_.get("Compression"), self_.get("SubfileType"));
    let mut name = Value::Undef;
    if out.truthy() {
        name = if pl::str_eq(&dir_name, "MakerNotes") && dir_info.get("TagInfo").truthy() { viv(&dir_info, "TagInfo").get("Name") } else { dir_name.clone() };
        if old_dir.is_undef() || !eq(&old_dir, &name) {
            let verb = if is_rewriting { "Rewriting" } else { "Creating" };
            print_out(it, &out, cat(&[&s(&format!("  {verb} ")), &name, &s("\n")]))?;
        }
    }
    let save_order = pl::call(it, "Image::ExifTool::GetByteOrder", vec![])?;
    let old_changed = self_.get("CHANGED");
    self_.set("DIR_NAME", dir_name.clone());
    viv_array(&self_, "PATH").push(dir_name.clone());
    dir_info.set("IsWriting", n(1));
    let new_data = pl::call_code(it, &write_proc, vec![self_.clone(), dir_info.clone(), tag_table_ptr.clone()], Want::Scalar)?.pop().unwrap_or_default();
    if let Value::ArrayRef(p) = viv_array(&self_, "PATH") {
        p.borrow_mut().pop();
    }
    // nothing changed if error occurred or nothing was created
    if !(!new_data.is_undef() && (length_of(&new_data) > 0 || is_rewriting)) {
        self_.set("CHANGED", old_changed.clone());
    }
    self_.set("DIR_NAME", old_dir);
    self_.set("Compression", save.0);
    self_.set("SubfileType", save.1);
    call_void(it, "Image::ExifTool::SetByteOrder", vec![save_order])?;
    if out.truthy() {
        if !new_data.is_undef() && length_of(&new_data) == 0 {
            print_out(it, &out, cat(&[&s("  Deleting "), &name, &s("\n")]))?;
        }
        if pl::num_eq(&self_.get("CHANGED"), &old_changed) && pl::gt(&opt(&self_, "Verbose"), &n(2)) {
            print_out(it, &out, cat(&[&self_.get("INDENT"), &s("  [nothing changed in "), &name, &s("]\n")]))?;
        }
    }
    ret(new_data)
}

/// `$self->Warn(MSG)`
fn warn_(it: &mut Interp, self_: &Value, msg: Value) -> ER<Value> {
    pl::method(it, self_, "Warn", vec![msg])
}

/// `$self->Error(MSG)`
fn error_(it: &mut Interp, self_: &Value, msg: Value) -> ER<Value> {
    pl::method(it, self_, "Error", vec![msg])
}

/// `warn "$err\n"` (the message as bytes, through `$SIG{__WARN__}`).
fn warn_nl(it: &mut Interp, err: &Value) -> ER<()> {
    it.warn(pl::concat(err, &s("\n")))
}

/// Set filesystem time from from FileModifyDate or FileCreateDate tag
/// Inputs: 0) ExifTool object reference, 1) file name or file ref
///         2) time (-M or -C) of original file (used for shift; obtained from file if not given)
///         3) tag name to write (undef for 'FileModifyDate')
///         4) flag set if argument 2 has already been converted to Unix seconds
/// Returns: 1=time changed OK, 0=nothing done, -1=error setting time
///          (increments CHANGED flag and sets corresponding WRITTEN tag)
pub fn set_file_modify_date(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (self_, file, mut original_time, mut tag, mut is_unix_time) = (arg(&a, 0), arg(&a, 1), arg(&a, 2), arg(&a, 3), arg(&a, 4));
    let mut nv_hash = Value::Undef;
    if tag.is_undef() {
        tag = s("FileModifyDate");
    }
    let mut val = get_new_value_nvh(it, &self_, tag.clone(), &mut nv_hash)?;
    if val.is_undef() {
        return ret(n(0));
    }
    let is_overwriting = overwriting(it, &self_, &nv_hash, None)?;
    if !is_overwriting.truthy() {
        return ret(n(0));
    }
    // can currently only set creation date on Windows systems
    // (and Mac now too, but that is handled with the MacOS tags)
    if pl::str_eq(&tag, "FileCreateDate") && !pl::str_eq(&os(it), "MSWin32") {
        return ret(n(0));
    }
    if pl::lt(&is_overwriting, &n(0)) {
        // are we shifting time?
        // use original time of this file if not specified
        if original_time.is_undef() {
            let t = pl::method_list(it, &self_, "GetFileTime", vec![file.clone()])?;
            let (_a_time, m_time, c_time) = (arg(&t, 0), arg(&t, 1), arg(&t, 2));
            original_time = if pl::str_eq(&tag, "FileCreateDate") { c_time } else { m_time };
            if original_time.is_undef() {
                return ret(n(0));
            }
            is_unix_time = n(1);
        }
        if !is_unix_time.truthy() {
            let t0 = it.get_scalar("main::^T");
            original_time = pl::int(&pl::add(&pl::sub(&t0, &pl::mul(&original_time, &n(24 * 3600))), &Value::Num(0.5)));
        }
        if !overwriting(it, &self_, &nv_hash, Some(original_time.clone()))?.truthy() {
            return ret(n(0));
        }
        val = nv_hash.get("Value").at(0); // get shifted value
    }
    let (mut a_time, mut m_time, mut c_time) = (Value::Undef, Value::Undef, Value::Undef);
    if pl::str_eq(&tag, "FileCreateDate") {
        if pl::eval_block(it, |it| crate::runtime::require(it, "Win32::API"))?.is_none() {
            warn_(it, &self_, cat(&[&s("Install Win32::API to set "), &tag]))?;
            return ret(n(-1));
        }
        if pl::eval_block(it, |it| crate::runtime::require(it, "Win32API::File"))?.is_none() {
            warn_(it, &self_, cat(&[&s("Install Win32API::File to set "), &tag]))?;
            return ret(n(-1));
        }
        c_time = val.clone();
    } else {
        a_time = val.clone();
        m_time = val.clone();
    }
    if !pl::method(it, &self_, "SetFileTime", vec![file, a_time, m_time, c_time, n(1)])?.truthy() {
        warn_(it, &self_, cat(&[&s("Error setting "), &tag]))?;
        return ret(n(-1));
    }
    pre_inc(&self_, "CHANGED");
    viv(&self_, "WRITTEN").set(tag.to_bytes(), val.clone()); // remember that we wrote this tag
    method_void(it, &self_, "VerboseValue", vec![pl::concat(&s("+ "), &tag), val])?;
    ret(n(1))
}

fn file_modify_date(it: &mut Interp, args: Vec<Value>) -> ER<Value> {
    Ok(set_file_modify_date(it, args, Want::Scalar)?.pop().unwrap_or_default())
}

/// Change file name and/or directory from FileName and Directory tags
/// Inputs: 0) ExifTool object reference, 1) current file name (including path)
///         2) new name (or undef to build from FileName and Directory tags)
///         3) option: 'HardLink'/'SymLink' to create hard/symbolic link instead of renaming
///                    'Test' to only print new file name
///         4) 0 to indicate that a file will no longer exist (used for 'Test' only)
/// Returns: 1=name changed OK, 0=nothing changed, -1=error changing name
///          (and increments CHANGED flag if filename changed)
/// Notes: Will not overwrite existing file.  Creates directories as necessary.
pub fn set_file_name(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (self_, mut file, mut new_name, mut opt_, used_flag) = (arg(&a, 0), arg(&a, 1), arg(&a, 2), arg(&a, 3), arg(&a, 4));
    let mut nv_hash = Value::Undef;
    let (mut do_name, mut do_dir) = (false, false);

    if !opt_.truthy() {
        opt_ = s("");
    }
    // determine the new file name
    if new_name.is_undef() {
        if opt_.truthy() {
            if pl::str_eq(&opt_, "HardLink") || pl::str_eq(&opt_, "Link") {
                new_name = get_new_value_s(it, &self_, s("HardLink"))?;
            } else if pl::str_eq(&opt_, "SymLink") {
                new_name = get_new_value_s(it, &self_, s("SymLink"))?;
            } else if pl::str_eq(&opt_, "Test") {
                new_name = get_new_value_s(it, &self_, s("TestName"))?;
            }
            if new_name.is_undef() {
                return ret(n(0));
            }
        } else {
            let filename = get_new_value_nvh(it, &self_, s("FileName"), &mut nv_hash)?;
            if !filename.is_undef() && overwriting(it, &self_, &nv_hash, Some(file.clone()))?.truthy() {
                do_name = true;
            }
            let dir = get_new_value_nvh(it, &self_, s("Directory"), &mut nv_hash)?;
            if !dir.is_undef() && overwriting(it, &self_, &nv_hash, Some(file.clone()))?.truthy() {
                do_dir = true;
            }
            if !(do_name || do_dir) {
                return ret(n(0)); // nothing to do
            }
            if do_name {
                new_name = new_file_name(it, &file, &filename)?;
                if do_dir {
                    new_name = new_file_name(it, &new_name, &dir)?;
                }
            } else {
                new_name = new_file_name(it, &file, &dir)?;
            }
        }
    }
    // validate new file name in Windows
    if pl::str_eq(&os(it), "MSWin32") {
        if re(r#"[\0-\x1f<>"|*]"#, "").is_match(&new_name) {
            warn_(it, &self_, s("New file name not allowed in Windows (contains reserved characters)"))?;
            return ret(n(-1));
        }
        if re(r":", "").is_match(&new_name) && !re(r"^[A-Z]:[^:]*$", "i").is_match(&new_name) {
            warn_(it, &self_, s("New file name not allowed in Windows (contains ':')"))?;
            return ret(n(-1));
        }
        if re(r"\?", "").is_match(&new_name) && !re(r"^[\\/]{2}\?[\\/][^?]*$", "").is_match(&new_name) {
            warn_(it, &self_, s("New file name not allowed in Windows (contains '?')"))?;
            return ret(n(-1));
        }
        if re(r"(^|[\\/])(CON|PRN|AUX|NUL|COM[1-9]|LPT[1-9])(\.[^.]*)?$", "i").is_match(&new_name) {
            warn_(it, &self_, s("New file name not allowed in Windows (reserved device name)"))?;
            return ret(n(-1));
        }
        if let Some(c) = re(r"([. ])$", "").m(&new_name) {
            if pl::method(it, &self_, "Warn", vec![cat(&[&s("New file name not recommended for Windows (ends with '"), &cap(&c, 1), &s("')")]), n(2)])?.truthy() {
                return ret(n(-1));
            }
        }
        if length_of(&new_name) > 259 && !re(r"\?", "").is_match(&new_name) && pl::method(it, &self_, "Warn", vec![s("New file name not recommended for Windows (exceeds 260 chars)"), n(2)])?.truthy() {
            return ret(n(-1));
        }
    } else {
        pl::tr(&mut new_name, "\0", "", "d"); // make sure name doesn't contain nulls
    }
    // protect against empty file name
    if length_of(&new_name) == 0 {
        warn_(it, &self_, s("New file name is empty"))?;
        return ret(n(-1));
    }
    // don't replace existing file
    if pl::method(it, &self_, "Exists", vec![new_name.clone(), n(1)])?.truthy() && (used_flag.is_undef() || used_flag.truthy()) {
        if !eq(&file, &new_name) || re(r"Link$", "").is_match(&opt_) {
            // allow for case-insensitive filesystem
            if re(r"Link$", "").is_match(&opt_) || !is_same_file(it, vec![self_.clone(), file.clone(), new_name.clone()], Want::Scalar)?.pop().unwrap_or_default().truthy() {
                warn_(it, &self_, cat(&[&s("File '"), &new_name, &s("' already exists")]))?;
                return ret(n(-1));
            }
        } else {
            warn_(it, &self_, s("File name is unchanged"))?;
            return ret(n(0));
        }
    }
    if pl::str_eq(&opt_, "Test") {
        let out = opt(&self_, "TextOut");
        print_out(it, &out, cat(&[&s("'"), &file, &s("' --> '"), &new_name, &s("'\n")]))?;
        return ret(n(1));
    }
    // create directory for new file if necessary
    let err = pl::method(it, &self_, "CreateDirectory", vec![new_name.clone()])?;
    if !err.is_undef() {
        if err.truthy() {
            if !re(r"^Error creating", "").is_match(&err) {
                warn_(it, &self_, err.clone())?;
            }
            warn_(it, &self_, cat(&[&s("Error creating directory for '"), &new_name, &s("'")]))?;
            return ret(n(-1));
        }
        method_void(it, &self_, "VPrint", vec![n(0), cat(&[&s("Created directory for '"), &new_name, &s("'\n")])])?;
    }
    if pl::str_eq(&opt_, "HardLink") || pl::str_eq(&opt_, "Link") {
        if !snippet1(it, "link $file, $newName", &[("file", file.clone()), ("newName", new_name.clone())])?.truthy() {
            warn_(it, &self_, cat(&[&s("Error creating hard link '"), &new_name, &s("'")]))?;
            return ret(n(-1));
        }
        pre_inc(&self_, "CHANGED");
        method_void(it, &self_, "VerboseValue", vec![s("+ HardLink"), new_name])?;
        return ret(n(1));
    } else if pl::str_eq(&opt_, "SymLink") {
        if pl::str_eq(&os(it), "MSWin32") {
            warn_(it, &self_, s("SymLink not supported in Windows"))?;
            return ret(n(-1));
        }
        re(r"^\./", "").subst(&mut new_name, false, |_| Vec::new()); // remove leading "./" from link name if it exists
        // path to linked file must be relative to the $newName directory, but $file
        // is relative to the current directory, so convert it to an absolute path
        // if using a relative directory and $newName isn't in the current directory
        if !re(r"^/", "").is_match(&file) && re(r"/", "").is_match(&new_name) {
            if pl::eval_block(it, |it| crate::runtime::require(it, "Cwd"))?.is_none() {
                warn_(it, &self_, s("Install Cwd to make symlinks to other directories"))?;
                return ret(n(-1));
            }
            let f = file.clone();
            file = pl::eval_block(it, |it| pl::call(it, "Cwd::abs_path", vec![f]))?.unwrap_or_default();
            if file.is_undef() {
                warn_(it, &self_, s("Error in Cwd::abs_path when creating symlink"))?;
                return ret(n(-1));
            }
        }
        if !snippet1(it, "eval { symlink $file, $newName }", &[("file", file.clone()), ("newName", new_name.clone())])?.truthy() {
            warn_(it, &self_, cat(&[&s("Error creating symbolic link '"), &new_name, &s("'")]))?;
            return ret(n(-1));
        }
        pre_inc(&self_, "CHANGED");
        method_void(it, &self_, "VerboseValue", vec![s("+ SymLink"), new_name])?;
        return ret(n(1));
    }
    // attempt to rename the file
    if !pl::method(it, &self_, "Rename", vec![file.clone(), new_name.clone()])?.truthy() {
        // local (*EXIFTOOL_SFN_IN, *EXIFTOOL_SFN_OUT);
        // renaming didn't work, so copy the file instead
        let fin = glob_ref(it, "EXIFTOOL_SFN_IN")?;
        if !pl::method(it, &self_, "Open", vec![fin, file.clone()])?.truthy() {
            error_(it, &self_, cat(&[&s("Error opening '"), &file, &s("'")]))?;
            return ret(n(-1));
        }
        let fout = glob_ref(it, "EXIFTOOL_SFN_OUT")?;
        if !pl::method(it, &self_, "Open", vec![fout, new_name.clone(), s(">")])?.truthy() {
            perl_snippet(it, "close EXIFTOOL_SFN_IN", &[], Want::Void)?;
            error_(it, &self_, cat(&[&s("Error creating '"), &new_name, &s("'")]))?;
            return ret(n(-1));
        }
        perl_snippet(it, "binmode EXIFTOOL_SFN_IN; binmode EXIFTOOL_SFN_OUT;", &[], Want::Void)?;
        let err = snippet1(
            it,
            "my ($buff, $err); while (read EXIFTOOL_SFN_IN, $buff, 65536) { print EXIFTOOL_SFN_OUT $buff or $err = 1; } close EXIFTOOL_SFN_OUT or $err = 1; close EXIFTOOL_SFN_IN; $err",
            &[],
        )?;
        if err.truthy() {
            method_void(it, &self_, "Unlink", vec![new_name.clone()])?; // erase bad output file
            error_(it, &self_, cat(&[&s("Error writing '"), &new_name, &s("'")]))?;
            return ret(n(-1));
        }
        // preserve modification time
        let t = pl::method_list(it, &self_, "GetFileTime", vec![file.clone()])?;
        method_void(it, &self_, "SetFileTime", vec![new_name.clone(), arg(&t, 0), arg(&t, 1), arg(&t, 2)])?;
        // remove the original file
        if !pl::method(it, &self_, "Unlink", vec![file.clone()])?.truthy() {
            warn_(it, &self_, s("Error removing old file"))?;
        }
    }
    self_.set("NewName", new_name.clone()); // remember new file name
    pre_inc(&self_, "CHANGED");
    method_void(it, &self_, "VerboseValue", vec![s("+ FileName"), new_name])?;
    ret(n(1))
}

fn file_name(it: &mut Interp, args: Vec<Value>) -> ER<Value> {
    Ok(set_file_name(it, args, Want::Scalar)?.pop().unwrap_or_default())
}

/// Set file permissions, group/user id and various MDItem tags from new tag values
/// Inputs: 0) ExifTool ref, 1) file name or glob (must be a name for MDItem tags)
/// Returns: 1=something was set OK, 0=didn't try, -1=error (and warning set)
/// Notes: There may be errors even if 1 is returned
pub fn set_system_tags(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (self_, file) = (arg(&a, 0), arg(&a, 1));
    let mut result = n(0);

    let perm = get_new_value_s(it, &self_, s("FilePermissions"))?;
    if !perm.is_undef() {
        if snippet1(it, "eval { chmod($perm & 07777, $file) }", &[("perm", perm.clone()), ("file", file.clone())])?.truthy() {
            method_void(it, &self_, "VerboseValue", vec![s("+ FilePermissions"), perm])?;
            result = n(1);
        } else {
            warn_(it, &self_, s("Error setting FilePermissions"))?;
            result = n(-1);
        }
    }
    let mut uid = get_new_value_s(it, &self_, s("FileUserID"))?;
    let mut gid = get_new_value_s(it, &self_, s("FileGroupID"))?;
    if !uid.is_undef() || !gid.is_undef() {
        if uid.is_undef() {
            uid = n(-1);
        }
        if gid.is_undef() {
            gid = n(-1);
        }
        if snippet1(it, "eval { chown($uid, $gid, $file) }", &[("uid", uid.clone()), ("gid", gid.clone()), ("file", file.clone())])?.truthy() {
            if pl::ge(&uid, &n(0)) {
                method_void(it, &self_, "VerboseValue", vec![s("+ FileUserID"), uid.clone()])?;
            }
            if pl::ge(&gid, &n(0)) {
                method_void(it, &self_, "VerboseValue", vec![s("+ FileGroupID"), gid.clone()])?;
            }
            result = n(1);
        } else {
            warn_(it, &self_, s("Error setting FileGroup/UserID"))?;
            if !result.truthy() {
                result = n(-1);
            }
        }
    }
    let writable_mac_os_tags = my_array(it, "writableMacOSTags");
    for tag in writable_mac_os_tags.items() {
        let mut nv_hash = Value::Undef;
        let _val = get_new_value_nvh(it, &self_, tag.clone(), &mut nv_hash)?;
        if !nv_hash.truthy() {
            continue;
        }
        if pl::str_eq(&os(it), "darwin") {
            if file.is_ref() {
                warn_(it, &self_, s("Setting MDItem tags requires a file name"))?;
                break;
            }
            crate::runtime::require(it, "Image::ExifTool::MacOS")?;
            let res = pl::call(it, "Image::ExifTool::MacOS::SetMacOSTags", vec![self_.clone(), file.clone(), writable_mac_os_tags.clone()])?;
            if pl::num_eq(&res, &n(1)) || !result.truthy() {
                result = res;
            }
            break;
        } else if !pl::str_eq(&tag, "FileCreateDate") {
            warn_(it, &self_, s("Can only set MDItem tags on MacOS"))?;
            break;
        }
    }
    // delete Windows Zone.Identifier if specified
    let zi = pl::pkg_hash(it, "Image::ExifTool::Extra").get("ZoneIdentifier");
    let mut zhash = gnvh(it, vec![self_.clone(), zi])?;
    if zhash.truthy() {
        let mut res = n(-1);
        if !pl::str_eq(&os(it), "MSWin32") {
            warn_(it, &self_, s("ZoneIdentifer is a Windows-only tag"))?;
        } else if file.is_ref() {
            warn_(it, &self_, s("Writing ZoneIdentifer requires a file name"))?;
        } else if !get_new_value_nvh(it, &self_, s("ZoneIdentifier"), &mut zhash)?.is_undef() {
            warn_(it, &self_, s("ZoneIndentifier may only be deleted"))?;
        } else if pl::eval_block(it, |it| crate::runtime::require(it, "Win32API::File"))?.is_none() {
            warn_(it, &self_, s("Install Win32API::File to write ZoneIdentifier"))?;
        } else {
            let mut wide = false;
            let mut zfile = [pl::concat(&file, &s(":Zone.Identifier"))];
            let wattr;
            if pl::method_mut(it, &self_, "EncodeFileName", &mut zfile, Want::Scalar)?.pop().unwrap_or_default().truthy() {
                wide = true;
                let z = zfile[0].clone();
                wattr = pl::eval_block(it, |it| pl::call(it, "Win32API::File::GetFileAttributesW", vec![z]))?.unwrap_or_default();
            } else {
                let z = zfile[0].clone();
                wattr = pl::eval_block(it, |it| pl::call(it, "Win32API::File::GetFileAttributes", vec![z]))?.unwrap_or_default();
            }
            let inv = pl::call(it, "Win32API::File::INVALID_FILE_ATTRIBUTES", vec![])?;
            let ro = pl::call(it, "Win32API::File::FILE_ATTRIBUTE_READONLY", vec![])?;
            if pl::num_eq(&wattr, &inv) {
                res = n(0); // file doesn't exist, nothing to do
            } else if ops::bit_and(&wattr, &ro).truthy() {
                warn_(it, &self_, s("Zone.Identifier stream is read-only"))?;
            } else {
                let z = zfile[0].clone();
                let ok = if wide {
                    pl::eval_block(it, |it| pl::call(it, "Win32API::File::DeleteFileW", vec![z]))?.unwrap_or_default()
                } else {
                    pl::eval_block(it, |it| pl::call(it, "Win32API::File::DeleteFile", vec![z]))?.unwrap_or_default()
                };
                if ok.truthy() {
                    res = n(1);
                }
                if pl::gt(&res, &n(0)) {
                    method_void(it, &self_, "VPrint", vec![n(0), s("  Deleting Zone.Identifier stream\n")])?;
                } else {
                    warn_(it, &self_, s("Error deleting Zone.Identifier stream"))?;
                }
            }
        }
        if pl::num_eq(&res, &n(1)) || !result.truthy() {
            result = res;
        }
    }
    ret(result)
}

fn system_tags(it: &mut Interp, self_: &Value, file: &Value) -> ER<Value> {
    Ok(set_system_tags(it, vec![self_.clone(), file.clone()], Want::Scalar)?.pop().unwrap_or_default())
}

/// `grep /^$tag(\s|$)/i, LIST`
fn grep_tag(tag: &Value, list: &[Value]) -> ER<Vec<Value>> {
    let mut p = b"^".to_vec();
    p.extend_from_slice(&tag.to_bytes());
    p.extend_from_slice(br"(\s|$)");
    grep_dyn(&p, "i", list)
}

/// Convert tag names to values or variables in a string
/// (eg. '${EXIF:ISO}x $$' --> '100x $' without hash ref, or "$info{'EXIF:ISO'}x $" with)
/// Inputs: 0) ExifTool object ref, 1) string with embedded tag names,
///         2) reference to list of found tags or undef to use FOUND_TAGS, 3) Options:
///               undef    - set missing tags to ''
///              'Error'   - issue minor error on missing tag (and return undef if error sent)
///              'Warn'    - issue minor warning on missing tag (and return undef if warning sent)
///              'Silent'  - just return undef on missing tag (no errors/warnings)
///               Hash ref - defined to interpolate as variables in string instead of values
///                          --> receives tag/value pairs for interpolation of the variables
///         4) document group name if extracting from a specific document
///         5) hash ref to cache tag keys for subsequent calls in document loop
/// Returns: string with embedded tag values (or '$info{TAGNAME}' entries with Hash ref option)
/// Notes:
/// - tag names are not case sensitive and may end with '#' for ValueConv value
/// - uses MissingTagValue option if set
/// - '$GROUP:all' evaluates to 1 if any tag from GROUP exists, or 0 otherwise
/// - advanced feature allows Perl expressions inside braces (eg. '${model;tr/ //d}')
/// - an error/warning in an advanced expression ("${TAG;EXPR}") generates an error
///   if option set to 'Error', or a warning otherwise
pub fn insert_tag_values(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    pl::local_topic(it, |it| insert_tag_values_body(it, a))
}

fn insert_tag_values_body(it: &mut Interp, a: Vec<Value>) -> ER<Vec<Value>> {
    let (self_, mut line, mut found_tags, opt_, doc_grp, mut cache) = (arg(&a, 0), arg(&a, 1), arg(&a, 2), arg(&a, 3), arg(&a, 4), arg(&a, 5));
    let mut rtn_str = s("");
    let mut doc_num = Value::Undef;
    let mut tag;

    if doc_grp.truthy() {
        doc_num = match re(r"(\d+(-\d+)*)$", "").m(&doc_grp) {
            Some(c) => cap(&c, 1),
            None => n(0),
        };
    } else {
        cache = Value::Undef; // no cache if no document groups
    }
    if !found_tags.truthy() {
        found_tags = self_.get("FOUND_TAGS");
        if !found_tags.truthy() {
            found_tags = Value::new_array(Vec::new());
        }
    }
    loop {
        let mut c3: Option<pl::Caps> = None;
        if re(r"(.*?)\$(\{\s*)?([-\w]*\w|\$|\/)", "s").subst(&mut line, false, |c| {
            c3 = Some(c.clone());
            Vec::new()
        }) == 0
        {
            break;
        }
        let c = c3.unwrap();
        let (pre, bra, mut var) = (cap(&c, 1), cap(&c, 2), cap(&c, 3));
        let mut tags: Vec<Value> = Vec::new();
        let mut val = Value::Undef;
        let val_arr = Value::new_array(Vec::new()); // @val
        let mut type_ = Value::Undef;
        let mut expr = Value::Undef;
        let mut did_expr = false;
        let mut as_list = false;
        // "$$" represents a "$" symbol, and "$/" is a newline
        if pl::str_eq(&var, "$") || pl::str_eq(&var, "/") {
            if bra.truthy() {
                re(r"^\s*\}", "").subst(&mut line, false, |_| Vec::new());
            }
            if pl::str_eq(&var, "/") {
                var = s("\n");
            } else if re(r"^self\b", "").is_match(&line) && !re(r"\$$", "").is_match(&rtn_str) {
                var = s("$$"); // ("$$self{var}" in string)
            }
            rtn_str = cat(&[&rtn_str, &pre, &var]);
            continue;
        }
        // allow multiple group names
        while let Some(m) = re(r"^:([-\w]*\w)(.*)", "s").m(&line) {
            let group = var.clone();
            var = cap(&m, 1);
            line = cap(&m, 2);
            var = cat(&[&group, &s(":"), &var]);
        }
        // allow trailing '#' to indicate ValueConv value
        if re(r"^#", "").subst(&mut line, false, |_| Vec::new()) > 0 {
            type_ = s("ValueConv");
        }
        // special advanced formatting '@' feature to evaluate list values separately
        if bra.truthy() {
            let mut hash = Value::Undef;
            if re(r"^\@(#)?", "").subst(&mut line, false, |c| {
                hash = cap(c, 1);
                Vec::new()
            }) > 0
            {
                as_list = true;
                if hash.truthy() {
                    type_ = s("ValueConv");
                }
            }
        }
        // remove trailing bracket if there was a leading one
        // and extract Perl expression from inside brackets if it exists
        if bra.truthy() && re(r"^\s*\}", "").subst(&mut line, false, |_| Vec::new()) == 0 {
            let mut part = Value::Undef;
            if re(r"^\s*;\s*(.*?)\s*\}", "s").subst(&mut line, false, |c| {
                part = cap(c, 1);
                Vec::new()
            }) > 0
            {
                expr = s("");
                let mut level: i64 = 0;
                loop {
                    // increase nesting level for each opening brace
                    level += part.to_bytes().iter().filter(|&&b| b == b'{').count() as i64;
                    expr = pl::concat(&expr, &part);
                    if level == 0 {
                        break;
                    }
                    let mut next = Value::Undef;
                    if re(r"^(.*?)\s*\}", "s").subst(&mut line, false, |c| {
                        next = cap(c, 1);
                        Vec::new()
                    }) == 0
                    {
                        break; // get next part
                    }
                    part = next;
                    expr = pl::concat(&expr, &s("}")); // this brace was part of the expression
                    level -= 1;
                }
                // use default Windows filename filter if expression is empty
                if length_of(&expr) == 0 {
                    expr = s(r#"tr(/\\?*:|"<>\0)()d"#);
                }
            }
        }
        tags.push(var.clone());
        let tr = Value::new_array(tags);
        call_void(it, "Image::ExifTool::ExpandShortcuts", vec![tr.clone()])?;
        let mut tags: std::collections::VecDeque<Value> = tr.items().into();
        if tags.is_empty() {
            rtn_str = pl::concat(&rtn_str, &pre);
            continue;
        }
        // save advanced formatting expression to allow access by user-defined ValueConv
        self_.set("FMT_EXPR", expr.clone());

        loop {
            // temporarily reset ListJoin option if evaluating list values separately
            let mut old_list_join = Value::Undef;
            if as_list {
                old_list_join = pl::method(it, &self_, "Options", vec![s("ListJoin"), Value::Undef])?;
            }
            tag = tags.pop_front().unwrap_or_default();
            let mut lc_tag = lc(&tag);
            if cache.truthy() && !re(r"(^|:)all$", "").is_match(&lc_tag) {
                // remove group from tag name (but not lower-case version)
                let mut group = Value::Undef;
                re(r"^(.*):", "").subst(&mut tag, false, |c| {
                    group = cap(c, 1);
                    Vec::new()
                });
                // cache tag keys to speed processing for a large number of sub-documents
                // (similar to code in BuildCompositeTags(), but this is case-insensitive)
                let mut cache_tag = cache.get(lc_tag.to_bytes());
                if !cache_tag.truthy() {
                    cache_tag = Value::new_hash();
                    cache.set(lc_tag.to_bytes(), cache_tag.clone());
                    // find all matching keys, organize into groups, and store in cache
                    let ex = self_.get("TAG_EXTRA");
                    let mut matches = grep_tag(&tag, &found_tags.items())?;
                    if !group.is_undef() {
                        matches = pl::method_list(it, &self_, "GroupMatches", vec![group.clone(), Value::new_array(matches)])?;
                    }
                    for m in matches {
                        let g3 = viv(&ex, m.to_bytes()).get("G3");
                        let doc = if g3.truthy() { g3 } else { n(0) };
                        let cur_v = cache_tag.get(doc.to_bytes());
                        if !cur_v.is_undef() {
                            let Some(cc) = re(r" \((\d+)\)$", "").m(&cur_v) else { continue };
                            let cur = cap(&cc, 1);
                            // keep the most recently extracted tag
                            if let Some(mc) = re(r" \((\d+)\)$", "").m(&m) {
                                if pl::lt(&cap(&mc, 1), &cur) {
                                    continue;
                                }
                            }
                        }
                        cache_tag.set(doc.to_bytes(), m);
                    }
                }
                let doc = match re(r"\b(main|doc(\d+(-\d+)*)):", "").m(&lc_tag) {
                    Some(c) => {
                        let d = cap(&c, 2);
                        if d.truthy() {
                            d
                        } else {
                            n(0)
                        }
                    }
                    None => doc_num.clone(),
                };
                if cache_tag.get(doc.to_bytes()).truthy() {
                    tag = cache_tag.get(doc.to_bytes());
                    val = pl::method(it, &self_, "GetValue", vec![tag.clone(), type_.clone()])?;
                }
            } else {
                // add document number to tag if specified and it doesn't already exist
                if doc_grp.truthy() && !re(r"\b(main|doc\d+):", "").is_match(&lc_tag) {
                    tag = cat(&[&doc_grp, &s(":"), &tag]);
                    lc_tag = lc(&tag);
                }
                let (mut et, mut file_tags) = (self_.clone(), found_tags.clone());
                let mut f1 = Value::Undef;
                if re(r"(\bfile\d+):", "i").subst(&mut tag, false, |c| {
                    f1 = cap(c, 1);
                    Vec::new()
                }) > 0
                {
                    et = viv(&self_, "ALT_EXIFTOOL").get(pl::ucfirst(&lc(&f1)).to_bytes());
                    if et.truthy() {
                        file_tags = et.get("FoundTags");
                    } else {
                        et = self_.clone();
                        tag = s("no_alt_file");
                    }
                }
                let user_param = viv(&viv(&et, "OPTIONS"), "UserParam").get(lc_tag.to_bytes());
                if pl::str_eq(&lc_tag, "all") {
                    val = n(1); // always some tag available
                } else if !user_param.is_undef() {
                    val = user_param;
                } else if let Some(m) = re(r"(.*):(.+)", "").m(&tag) {
                    let mut group = cap(&m, 1);
                    tag = cap(&m, 2);
                    let matches: Vec<Value>;
                    // join values of all matching tags if "All" group is used
                    // (and remove "All" from group prefix)
                    if re(r"(^|:)(all|\*)(:|$)", "i").subst(&mut group, false, |c| {
                        let one = cap(c, 1);
                        if one.truthy() {
                            cap(c, 3).to_bytes().to_vec()
                        } else {
                            one.to_bytes().to_vec()
                        }
                    }) > 0
                    {
                        if pl::str_eq(&lc(&tag), "all") {
                            matches = if group.truthy() { pl::method_list(it, &et, "GroupMatches", vec![group.clone(), file_tags.clone()])? } else { file_tags.items() };
                        } else {
                            let mut m2 = grep_tag(&tag, &file_tags.items())?;
                            if group.truthy() {
                                m2 = pl::method_list(it, &et, "GroupMatches", vec![group.clone(), Value::new_array(m2)])?;
                            }
                            matches = m2;
                        }
                        for m in matches {
                            let v = pl::method(it, &et, "GetValue", vec![m, type_.clone()])?;
                            push_value(it, vec![self_.clone(), v, val_arr.clone()], Want::Void)?;
                        }
                    } else if pl::str_eq(&lc(&tag), "all") {
                        // return "1" if any tag from the specified group exists
                        val = if pl::method(it, &et, "GroupMatches", vec![group.clone(), file_tags.clone()])?.truthy() { n(1) } else { n(0) };
                    } else {
                        // find the specified tag
                        let m2 = grep_tag(&tag, &file_tags.items())?;
                        let m2 = pl::method_list(it, &et, "GroupMatches", vec![group.clone(), Value::new_array(m2)])?;
                        for tg in m2 {
                            if !val.is_undef() {
                                if let Some(c) = re(r" \((\d+)\)$", "").m(&tg) {
                                    // take the most recently extracted tag
                                    let tag_num = cap(&c, 1);
                                    match re(r" \((\d+)\)$", "").m(&tag) {
                                        None => continue,
                                        Some(c2) => {
                                            if pl::gt(&cap(&c2, 1), &tag_num) {
                                                continue;
                                            }
                                        }
                                    }
                                }
                            }
                            val = pl::method(it, &et, "GetValue", vec![tg.clone(), type_.clone()])?;
                            tag = tg;
                            if !re(r" ", "").is_match(&tag) {
                                break; // all done if we got our best match
                            }
                        }
                    }
                } else if pl::str_eq(&tag, "self") {
                    val = et.clone(); // ("$self{var}" or "$file1:self{var}" in string)
                } else {
                    // get the tag value (note: this direct access allows excluded tags
                    // to be accessed if the case is correct and a group name is not used)
                    val = pl::method(it, &et, "GetValue", vec![tag.clone(), type_.clone()])?;
                    if val.is_undef() {
                        // check for tag name with different case
                        let tg = grep_dyn(&anchored(&tag), "i", &file_tags.items())?.into_iter().next();
                        if let Some(tg) = tg {
                            val = pl::method(it, &et, "GetValue", vec![tg.clone(), type_.clone()])?;
                            tag = tg;
                        }
                    }
                }
            }
            if as_list {
                method_void(it, &self_, "Options", vec![s("ListJoin"), old_list_join])?;
            }
            push_value(it, vec![self_.clone(), val.clone(), val_arr.clone(), Value::from(as_list)], Want::Void)?;
            val = Value::Undef;
            if tags.is_empty() {
                break;
            }
        }
        if !val_arr.is_empty() {
            if !val.is_undef() {
                push_value(it, vec![self_.clone(), val.clone(), val_arr.clone()], Want::Void)?;
            }
            val = Value::bytes(val_arr.items().iter().map(|x| x.to_bytes().to_vec()).collect::<Vec<_>>().join(&opt(&self_, "ListSep").to_bytes()[..]));
        } else if !val.is_undef() {
            push_value(it, vec![self_.clone(), val.clone(), val_arr.clone()], Want::Void)?; // (so the eval has access to @val if required)
        }
        // evaluate advanced formatting expression if given (eg. "${TAG;EXPR}")
        if !expr.is_undef() && !val.is_undef() {
            let h = set_warning_ref(it);
            let sep = opt(&self_, "ListSep").to_bytes();
            let tag_now = tag.clone();
            val = pl::local_sig_warn(it, h, |it| {
                it.set_scalar(EVAL_WARNING, Value::Undef);
                it.set_scalar(ADV_FMT_SELF, self_.clone()); // set variable for access to $self in helper functions
                let scope = Scope::new(Some(writer_scope(it)));
                scope.scalar("self", self_.clone());
                scope.scalar("tag", tag_now.clone());
                scope.scalar("val", val.clone());
                scope.scalar("var", var.clone());
                scope.scalar("expr", expr.clone());
                scope.scalar("opt", opt_.clone());
                if let Value::ArrayRef(arr) = &val_arr {
                    scope.declare(b'@', "val", Binding::Array(arr.clone()));
                }
                let new_val;
                if as_list {
                    let saved = it.get_scalar("main::_");
                    for i in 0..val_arr.len() {
                        it.set_scalar("main::_", val_arr.at(i as i64));
                        //### eval advanced formatting expression ($_, $self, @val, $tag, $advFmtSelf)
                        eval_in(it, &expr, &scope, Want::Scalar)?;
                        let e = eval_error(it);
                        if e.truthy() {
                            it.set_scalar(EVAL_WARNING, e);
                        }
                        let t = it.get_scalar("main::_");
                        if i < val_arr.len() {
                            val_arr.set_at(i, t);
                        }
                    }
                    it.set_scalar("main::_", saved);
                    // join back together if any values are still defined
                    let items: Vec<Value> = val_arr.items().into_iter().filter(|v| !v.is_undef()).collect();
                    array_assign(&val_arr, items.clone());
                    new_val = if items.is_empty() { Value::Undef } else { Value::bytes(items.iter().map(|x| x.to_bytes().to_vec()).collect::<Vec<_>>().join(&sep[..])) };
                } else {
                    it.set_scalar("main::_", val.clone());
                    //### eval advanced formatting expression ($_, $self, @val, $tag, $advFmtSelf)
                    eval_in(it, &expr, &scope, Want::Scalar)?;
                    let e = eval_error(it);
                    if e.truthy() {
                        it.set_scalar(EVAL_WARNING, e);
                    }
                    let t = it.get_scalar("main::_");
                    new_val = if t.ref_type() == "ARRAY" { Value::bytes(t.items().iter().map(|x| x.to_bytes().to_vec()).collect::<Vec<_>>().join(&sep[..])) } else { t };
                }
                if it.get_scalar(EVAL_WARNING).truthy() {
                    let g3 = if doc_grp.truthy() && !re(r"\b(main|doc\d+):", "i").is_match(&var) { pl::concat(&doc_grp, &s(":")) } else { s("") };
                    let str = cat(&[&clean_warning(it)?, &s(" for '"), &g3, &var, &s("'")]);
                    if opt_.truthy() {
                        if pl::str_eq(&opt_, "Error") {
                            error_(it, &self_, str)?;
                        } else if !pl::str_eq(&opt_, "Silent") {
                            warn_(it, &self_, str)?;
                        }
                    }
                }
                it.set_scalar(ADV_FMT_SELF, Value::Undef);
                Ok(new_val)
            })?;
            did_expr = true; // set flag indicating an expression was evaluated
        }
        if !(!val.is_undef() || (opt_.is_ref() && opt(&self_, "UndefTags").truthy())) {
            val = opt(&self_, "MissingTagValue");
            if val.is_undef() {
                let g3 = if doc_grp.truthy() && !re(r"\b(main|doc\d+):", "i").is_match(&var) { pl::concat(&doc_grp, &s(":")) } else { s("") };
                let msg = if did_expr {
                    cat(&[&s("Advanced formatting expression returned undef for '"), &g3, &var, &s("'")])
                } else {
                    cat(&[&s("Tag '"), &g3, &var, &s("' not defined")])
                };
                if opt_.is_ref() {
                    if opt(&self_, "IgnoreMinorErrors").truthy() {
                        val = s("");
                    }
                } else if opt_.truthy() {
                    if pl::str_eq(&opt_, "Silent") || pl::call(it, &format!("{ET}::{}", opt_.to_text()), vec![self_.clone(), msg, n(2)])?.truthy() {
                        self_.set("FMT_EXPR", Value::Undef);
                        return pl::ret_undef();
                    }
                    val = s("");
                }
            }
        }
        if opt_.ref_type() == "HASH" {
            if type_.truthy() {
                var = pl::concat(&var, &s("#"));
            }
            if !expr.is_undef() {
                // generate unique variable name for this modified tag value
                let mut i = 1;
                while opt_.exists(cat(&[&var, &s(&format!(".expr{i}"))]).to_bytes()) {
                    i += 1;
                }
                var = cat(&[&var, &s(&format!(".expr{i}"))]);
            }
            rtn_str = cat(&[&rtn_str, &pre, &s("$info{'"), &var, &s("'}")]);
            opt_.set(var.to_bytes(), val);
        } else {
            rtn_str = cat(&[&rtn_str, &pre, &val]);
        }
    }
    self_.set("FMT_EXPR", Value::Undef);
    ret(pl::concat(&rtn_str, &line))
}

/// `%$h` in list context: the key/value pairs.
fn hash_pairs(h: &Value) -> Vec<Value> {
    let mut v = Vec::new();
    if let Value::HashRef(r) = h {
        for (k, x) in r.borrow().iter() {
            v.push(Value::Str(k.clone()));
            v.push(x.clone());
        }
    }
    v
}

/// `$self->SetNewValue(TAG, VALUE, %opts)` in list context: (numSet, err).
fn snv(it: &mut Interp, self_: &Value, tag: Value, value: Value, opts: &Value) -> ER<(Value, Value)> {
    let mut args = vec![self_.clone(), tag, value];
    args.extend(hash_pairs(opts));
    let r = set_new_value(it, args, Want::List)?;
    Ok((arg(&r, 0), arg(&r, 1)))
}

/// `ExpandShortcuts(\@list[, $removeSuffix])`: the list as it leaves it.
fn expand_shortcuts(it: &mut Interp, list: Vec<Value>, extra: Option<Value>) -> ER<Vec<Value>> {
    let r = Value::new_array(list);
    let mut args = vec![r.clone()];
    if let Some(x) = extra {
        args.push(x);
    }
    call_void(it, "Image::ExifTool::ExpandShortcuts", args)?;
    Ok(r.items())
}

/// set new values from information in specified file
/// Inputs: 0) ExifTool object reference, 1) source file name or reference, etc,
///         or ExifTool ref to use already-extracted tags from an ExifTool object,
///         2-N) List of tags to set (or all if none specified), or reference(s) to
///         hash for options to pass to SetNewValue.  The Replace option defaults
///         to 1 for SetNewValuesFromFile -- set this to 0 to allow multiple tags
///         to be copied to a list
/// Returns: Hash of information set successfully (includes Warning or Error messages)
/// Notes: Tag names may contain a group prefix, a leading '-' to exclude from copy,
///        and/or a trailing '#' to copy the ValueConv value.  The tag name '*' may
///        be used to represent all tags in a group.  An optional destination tag
///        may be specified with '>DSTTAG' ('DSTTAG<TAG' also works, but in this
///        case the source tag may also be an expression involving tag names).
///        Simple assignments are also allowed: 'DSTTAG[#][+-][^]=[string]'
pub fn set_new_values_from_file(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    pl::local_topic(it, |it| set_new_values_from_file_body(it, a))
}

fn set_new_values_from_file_body(it: &mut Interp, a: Vec<Value>) -> ER<Vec<Value>> {
    let self_ = arg(&a, 0);
    let src_file = arg(&a, 1);
    let mut set_tags: Vec<Value> = a.iter().skip(2).cloned().collect();
    let src_exif_tool;
    let mut exclude: Vec<Value> = Vec::new();
    let mut req_tags: Vec<Value> = Vec::new();
    let info;

    // get initial SetNewValuesFromFile options
    let opts = Value::new_hash();
    opts.set("Replace", n(1)); // replace existing list items by default
    while set_tags.first().is_some_and(|t| t.ref_type() == "HASH") {
        let h = set_tags.remove(0);
        it.set_scalar("main::_", h.clone());
        for key in keys(&h) {
            opts.set(key.to_bytes(), h.get(key.to_bytes()));
        }
    }
    // expand shortcuts
    if !set_tags.is_empty() {
        set_tags = expand_shortcuts(it, set_tags, None)?;
    }
    // set options for our extraction tool
    let options = self_.get("OPTIONS");
    let mut print_conv = options.get("PrintConv");
    if opts.get("Type").truthy() {
        // save source type separately because it may be different than dst Type
        opts.set("SrcType", opts.get("Type"));
        // override PrintConv option with initial Type if given
        print_conv = if pl::str_eq(&opts.get("Type"), "PrintConv") { n(1) } else { n(0) };
    }
    let src_type = if print_conv.truthy() { s("PrintConv") } else { s("ValueConv") };
    let mut struct_opt = if !options.get("Struct").is_undef() { options.get("Struct") } else { n(2) };

    if src_file.is_ref() && pl::call(it, "UNIVERSAL::isa", vec![src_file.clone(), s(ET)])?.truthy() {
        src_exif_tool = src_file.clone();
        let o = Value::new_hash();
        o.set("PrintConv", print_conv.clone());
        info = pl::method(it, &src_exif_tool, "GetInfo", vec![o])?;
    } else {
        src_exif_tool = pl::method(it, &s(ET), "new", vec![])?;
        method_void(it, &src_exif_tool, "Options", vec![s("PrintConv"), print_conv.clone()])?;
        // set flag to indicate we are being called from inside SetNewValuesFromFile()
        src_exif_tool.set("TAGS_FROM_FILE", n(1));
        // synchronize and increment the file sequence number
        src_exif_tool.set("FILE_SEQUENCE", post_inc_raw(&self_, "FILE_SEQUENCE"));
        // copy both structured and flattened tags by default (but flattened tags are "unsafe")
        // copy structures only if no tags specified (since flattened tags are "unsafe")
        if pl::str_eq(&struct_opt, "2") && set_tags.is_empty() {
            struct_opt = n(1);
        }
        // +------------------------------------------+
        // ! DON'T FORGET!!  Must consider each new   !
        // ! option to decide how it is handled here. !
        // +------------------------------------------+
        for o in [
            "ByteUnit", "Charset", "CharsetEXIF", "CharsetFileName", "CharsetID3", "CharsetIPTC", "CharsetPhotoshop", "Composite", "DateFormat", "Debug", "EncodeHangs", "Escape", "ExtendedXMP", "ExtractEmbedded", "FastScan", "Filter", "FixBase", "Geolocation", "GeolocAltNames", "GeolocFeature",
            "GeolocMinPop", "GeolocMaxDist", "GlobalTimeShift", "GPSQuadrant", "HexTagIDs", "IgnoreGroups", "IgnoreMinorErrors", "IgnoreTags", "ImageHashType", "KeepUTCTime", "Lang", "LargeFileSupport", "LigoGPSScale", "ListItem", "ListSep", "MDItemTags", "MissingTagValue", "NoPDFList",
            "NoWarning", "Password", "PrintConv", "QuickTimeUTC", "RequestTags", "SaveFormat", "SavePath", "ScanForXMP", "StructFormat", "SystemTags", "SystemTimeRes", "TimeZone", "Unknown", "UserParam", "Validate", "WindowsLongPath", "WindowsWideFile", "XAttrTags", "XMPAutoConv",
        ] {
            it.set_scalar("main::_", s(o));
            method_void(it, &src_exif_tool, "Options", vec![s(o), options.get(o)])?;
        }
        let coord = options.get("CoordFormat");
        let fast_scan = options.get("FastScan");
        let req_all = options.get("RequestAll");
        let strict = options.get("StrictDate");
        method_void(
            it,
            &src_exif_tool,
            "Options",
            vec![
                s("Binary"),
                n(1),
                s("CoordFormat"),
                if coord.truthy() { coord } else { s("%d %d %.8f") }, // copy coordinates at high resolution unless otherwise specified
                s("Duplicates"),
                n(1),
                // Exclude (set below)
                s("LimitLongValues"),
                n(10000000), // (10 MB)
                s("List"),
                n(1),
                s("MakerNotes"),
                if fast_scan.truthy() && pl::gt(&fast_scan, &n(1)) { Value::Undef } else { n(1) },
                s("RequestAll"),
                if req_all.truthy() { req_all } else { n(1) }, // (must request all because reqTags doesn't cover wildcards)
                s("StrictDate"),
                if !strict.is_undef() { strict } else { n(1) },
                s("Struct"),
                struct_opt.clone(),
            ],
        )?;
        // reset Geolocation option if we aren't copying any geolocation tags
        if options.get("Geolocation").truthy() && !set_tags.iter().any(|t| re(r"\bGeolocation", "i").is_match(t)) {
            method_void(it, &self_, "VPrint", vec![n(0), s("(resetting unnecessary Geolocation option)")])?;
            viv(&src_exif_tool, "OPTIONS").set("Geolocation", Value::Undef);
        }
        src_exif_tool.set("GLOBAL_TIME_OFFSET", self_.get("GLOBAL_TIME_OFFSET"));
        src_exif_tool.set("ALT_EXIFTOOL", self_.get("ALT_EXIFTOOL"));
        for tag in &set_tags {
            if tag.is_ref() {
                continue;
            }
            // avoid extracting tags that are excluded
            if let Some(c) = re(r"^-(.*)", "").m(tag) {
                exclude.push(cap(&c, 1));
                continue;
            }
            // add specified tags to list of requested tags
            let mut t = tag.clone();
            if let Some(c) = re(r"(.+?)\s*(>|<)\s*(.+)", "").m(&t) {
                if pl::str_eq(&cap(&c, 2), ">") {
                    t = cap(&c, 1);
                } else {
                    t = cap(&c, 3);
                    if re(r"\$", "").is_match(&t) {
                        req_tags.extend(re(r"\$\{?(?:[-\w]+:)*([-\w?*]+)", "").all(&t));
                        it.set_scalar("main::_", t);
                        continue;
                    }
                }
            }
            if let Some(c) = re(r"(^|:)([-\w?*]+)#?$", "").m(&t) {
                req_tags.push(cap(&c, 2));
            }
            it.set_scalar("main::_", t);
        }
        if !exclude.is_empty() {
            exclude = expand_shortcuts(it, exclude, Some(n(1)))?;
            method_void(it, &src_exif_tool, "Options", vec![s("Exclude"), Value::new_array(exclude.clone())])?;
        }
        if !req_tags.is_empty() {
            method_void(it, &src_exif_tool, "Options", vec![s("RequestTags"), Value::new_array(req_tags.clone())])?;
        }
        // get all tags from source file (including MakerNotes block)
        info = pl::method(it, &src_exif_tool, "ImageInfo", vec![src_file.clone()])?;
    }
    // (allow processing to continue if we have alternate files that may have been read OK)
    if info.get("Error").truthy() && pl::str_eq(&info.get("Error"), "Error opening file") && !self_.get("ALT_EXIFTOOL").truthy() {
        return ret(info);
    }
    viv(&src_exif_tool, "VALUE").delete("Error"); // delete so we can check this later

    // sort tags in file order with priority tags last
    let (mut tags, mut prio) = (Vec::new(), Vec::new());
    let file_order = viv(&src_exif_tool, "FILE_ORDER");
    let mut ks = keys(&info);
    ks.sort_by(|x, y| ncmp(&file_order.get(x.to_bytes()), &file_order.get(y.to_bytes())));
    for k in ks {
        if re(r" ", "").is_match(&k) {
            tags.push(k);
        } else {
            prio.push(k);
        }
    }
    tags.extend(prio);
    //
    // simply transfer all tags from source image if no tags specified
    //
    if set_tags.is_empty() {
        // transfer maker note information to this object
        self_.set("MAKER_NOTE_BYTE_ORDER", src_exif_tool.get("MAKER_NOTE_BYTE_ORDER"));
        let tag_extra = src_exif_tool.get("TAG_EXTRA");
        for tag in &tags {
            // don't try to set errors or warnings
            if re(r"^(Error|Warning)\b", "").is_match(tag) {
                continue;
            }
            // get appropriate value type if necessary
            if opts.get("SrcType").truthy() && !eq(&opts.get("SrcType"), &src_type) {
                let v = pl::method(it, &src_exif_tool, "GetValue", vec![tag.clone(), opts.get("SrcType")])?;
                info.set(tag.to_bytes(), v);
            }
            let fixup = viv(&tag_extra, tag.to_bytes()).get("Fixup");
            if fixup.truthy() {
                opts.set("Fixup", fixup.clone());
            }
            // set value for this tag
            let (n_, _e) = snv(it, &self_, tag.clone(), info.get(tag.to_bytes()), &opts)?;
            // delete this tag if we couldn't set it
            if !n_.truthy() {
                info.delete(tag.to_bytes());
            }
            if fixup.truthy() {
                opts.delete("Fixup");
            }
        }
        return ret(info);
    }
    //
    // transfer specified tags in the proper order
    //
    // 1) loop through input list of tags to set, and build @setList
    let mut set_list: Vec<Value> = Vec::new();
    let alt_files = Value::new_hash();
    let mut assign = 0i64;
    for t in &set_tags {
        if t.ref_type() == "HASH" {
            // update current options
            for key in keys(t) {
                opts.set(key.to_bytes(), t.get(key.to_bytes()));
            }
            continue;
        }
        // make a copy of the current options for this setTag
        // (also use this hash to store expression and wildcard flags, EXPR and WILD)
        let opts2 = opts.hash_copy();
        let mut tag = lc(t); // change tag/group names to all lower case
        let mut fg: Vec<Value> = Vec::new();
        let mut grp = Value::Undef;
        let dst;
        let mut dst_grp = Value::Undef;
        let mut dst_tag = Value::Undef;
        let mut is_exclude = false;
        // handle redirection to another tag
        if let Some(c) = re(r"(.+?)\s*(>|<|=)(\s*)(.*)", "").m(&tag) {
            dst_grp = s("");
            let mut opt_ = Value::Undef;
            if pl::str_eq(&cap(&c, 2), ">") {
                tag = cap(&c, 1);
                dst_tag = cap(&c, 4);
                // flag add and delete (eg. '+<' and '-<') redirections
                let mut m1 = Value::Undef;
                if re(r"\s*([-+])$", "").subst(&mut tag, false, |c| {
                    m1 = cap(c, 1);
                    Vec::new()
                }) > 0
                    || re(r"^([-+])\s*", "").subst(&mut dst_tag, false, |c| {
                        m1 = cap(c, 1);
                        Vec::new()
                    }) > 0
                {
                    opt_ = m1;
                }
            } else {
                dst_tag = cap(&c, 1);
                let op = cap(&c, 2);
                let spc = cap(&c, 3);
                tag = cap(&c, 4);
                let mut m1 = Value::Undef;
                if re(r"\s*([-+])$", "").subst(&mut dst_tag, false, |c| {
                    m1 = cap(c, 1);
                    Vec::new()
                }) > 0
                {
                    opt_ = m1;
                }
                if pl::str_eq(&op, "=") {
                    // simple assignment ($tag will be the new value)
                    tag = pl::concat(&spc, &tag);
                    if !(re(r"\^$", "").subst(&mut dst_tag, false, |_| Vec::new()) > 0 || length_of(&tag) > 0) {
                        tag = Value::Undef;
                    }
                    assign += 1;
                    opts2.set("ASSIGN", n(assign));
                } else if re(r"\$", "").is_match(&tag) {
                    // handle expressions
                    tag = t.clone(); // restore original case
                    // recover leading whitespace (except for initial single space)
                    re(r"(.+?)\s*(>|<) ?", "").subst(&mut tag, false, |_| Vec::new());
                    opts2.set("EXPR", n(1)); // flag this expression
                } else {
                    // (not sure why this is here because sign should be before '<')
                    // (--> allows "<+" or "<-", which is an undocumented feature)
                    let mut m1 = Value::Undef;
                    if re(r"^([-+])\s*", "").subst(&mut tag, false, |c| {
                        m1 = cap(c, 1);
                        Vec::new()
                    }) > 0
                    {
                        opt_ = m1;
                    }
                }
            }
            if re(r"^\+", "").subst(&mut dst_tag, false, |_| Vec::new()) > 0 {
                opts2.set("Replace", n(0));
            }
            // validate tag name(s)
            if !(opts2.get("EXPR").truthy() || opts2.get("ASSIGN").truthy() || pl::call(it, "Image::ExifTool::ValidTagName", vec![tag.clone()])?.truthy()) {
                warn_(it, &self_, cat(&[&s("Invalid tag name '"), &tag, &s("'. Use '=' not '<' to assign a tag value")]))?;
                continue;
            }
            if !pl::call(it, "Image::ExifTool::ValidTagName", vec![dst_tag.clone()])?.truthy() {
                warn_(it, &self_, cat(&[&s("Invalid tag name '"), &dst_tag, &s("'")]))?;
                continue;
            }
            // translate '+' and '-' to appropriate SetNewValue option
            if opt_.truthy() {
                opts2.set(if pl::str_eq(&opt_, "+") { "AddValue" } else { "DelValue" }, n(1));
                opts2.set("Shift", n(0)); // shift if shiftable
            }
            if let Some(c) = re(r"(.*):(.+)", "").m(&dst_tag) {
                dst_grp = cap(&c, 1);
                dst_tag = cap(&c, 2);
            }
            // ValueConv may be specified separately on the destination with '#'
            if re(r"#$", "").subst(&mut dst_tag, false, |_| Vec::new()) > 0 {
                opts2.set("Type", s("ValueConv"));
            }
            // replace tag name of 'all' with '*'
            if pl::str_eq(&dst_tag, "all") {
                dst_tag = s("*");
            }
        } else if re(r"^\+", "").subst(&mut tag, false, |_| Vec::new()) > 0 {
            opts2.set("Replace", n(0));
        }
        if !(opts2.get("EXPR").truthy() || opts2.get("ASSIGN").truthy()) {
            is_exclude = re(r"^-", "").subst(&mut tag, false, |_| Vec::new()) > 0;
            if let Some(c) = re(r"(.*):(.+)", "").m(&tag) {
                grp = cap(&c, 1);
                tag = cap(&c, 2);
                for x in split_on(":", &grp) {
                    // save family/groups in list (ignoring 'all' and '*')
                    let Some(m) = re(r"^(\d+)?(.*)", "").m(&x) else { continue };
                    if length_of(&x) == 0 {
                        continue;
                    }
                    let (mut f, mut g) = (cap(&m, 1), cap(&m, 2));
                    if (!f.truthy() || pl::str_eq(&f, "7")) && re(r"^ID-", "i").subst(&mut g, false, |_| Vec::new()) > 0 {
                        f = n(7);
                    }
                    if re(r"^file\d+$", "i").is_match(&g) && (!f.truthy() || pl::str_eq(&f, "8")) {
                        f = n(8);
                        let g8 = pl::ucfirst(&g);
                        if viv(&src_exif_tool, "ALT_EXIFTOOL").get(g8.to_bytes()).truthy() {
                            opts2.set("GROUP8", g8.clone());
                            if !alt_files.get(g8.to_bytes()).truthy() {
                                alt_files.set(g8.to_bytes(), Value::new_array(Vec::new()));
                            }
                            // save list of requested tags for this alternate ExifTool object
                            alt_files.get(g8.to_bytes()).push(cat(&[&grp, &s(":"), &tag]));
                        }
                    }
                    if !(pl::str_eq(&g, "*") || pl::str_eq(&g, "all")) {
                        fg.push(Value::new_array(vec![f, g]));
                    }
                }
            }
            // allow ValueConv to be specified by a '#' on the tag name
            if re(r"#$", "").subst(&mut tag, false, |_| Vec::new()) > 0 {
                opts2.set("SrcType", s("ValueConv"));
                if !dst_tag.truthy() {
                    opts2.set("Type", s("ValueConv"));
                }
            }
            // replace 'all' with '*' in tag and group names
            if pl::str_eq(&tag, "all") {
                tag = s("*");
            }
            // allow wildcards in tag names (handle differently from all tags: '*')
            if re(r"[?*]", "").is_match(&tag) && !pl::str_eq(&tag, "*") {
                opts2.set("WILD", n(1)); // set flag indicating wildcards were used in source tag
                re(r"\*", "").subst(&mut tag, true, |_| br"[-\w]*".to_vec());
                re(r"\?", "").subst(&mut tag, true, |_| br"[-\w]".to_vec());
            }
        }
        // redirect, exclude or set this tag (Note: @fg is empty if we don't care about the group)
        if dst_tag.truthy() {
            // redirect this tag
            if is_exclude {
                let h = Value::new_hash();
                h.set("Error", s("Can't redirect excluded tag"));
                return ret(h);
            }
            // set destination group the same as source if necessary
            // (removed in 7.72 so '-*:*<xmp:*' will preserve XMP family 1 groups)
            // $dstGrp = $grp if $dstGrp eq '*' and $grp;
            // write to specified destination group/tag
            dst = Value::new_array(vec![dst_grp, dst_tag]);
        } else if is_exclude {
            // implicitly assume '*' if first entry is an exclusion
            if set_list.is_empty() {
                set_list.insert(0, Value::new_array(vec![Value::new_array(Vec::new()), s("*"), Value::new_array(vec![s(""), s("*")]), opts2.clone()]));
            }
            // exclude this tag by leaving $dst undefined
            dst = Value::Undef;
        } else {
            let g = if grp.truthy() { grp.clone() } else { s("") };
            dst = Value::new_array(vec![g, if opts2.get("WILD").truthy() { s("*") } else { tag.clone() }]); // use same group name for dest
        }
        // save in reverse order so we don't set tags before an exclude
        set_list.insert(0, Value::new_array(vec![Value::new_array(fg), tag, dst, opts2]));
    }
    // 1b) copy requested tags for each alternate ExifTool object
    for g8 in sort_str(keys(&alt_files)) {
        // request specific alternate tags to load them from the alternate ExifTool object
        let alt_info = pl::method(it, &src_exif_tool, "GetInfo", vec![alt_files.get(g8.to_bytes())])?;
        // add to tags list after dummy entry to signify start of tags for this alt file
        if !alt_info.is_empty() {
            tags.push(s("Warning DUMMY"));
            let mut k = sort_str(keys(&alt_info));
            k.reverse();
            tags.extend(k);
            for k in keys(&alt_info) {
                info.set(k.to_bytes(), alt_info.get(k.to_bytes()));
            }
        }
    }
    // 2) initialize lists of matching tags for each setTag
    let mut set_matches: std::collections::HashMap<Vec<u8>, Vec<Value>> = Default::default();
    for set in &set_list {
        if set.at(2).truthy() {
            set_matches.insert(set.to_bytes().to_vec(), Vec::new());
        }
    }
    // no need to search source tags if doing only assignments
    if assign as usize == set_list.len() {
        tags.clear();
    }
    // 3) loop through all tags in source image and save tags matching each setTag
    let rtn_info = Value::new_hash();
    let mut is_alt = false;
    for tag in &tags {
        // don't try to set errors or warnings
        if re(r"^(Error|Warning)( |$)", "").is_match(tag) {
            if pl::str_eq(tag, "Warning DUMMY") {
                is_alt = true; // start of the alt tags
            } else {
                rtn_info.set(tag.to_bytes(), info.get(tag.to_bytes()));
            }
            continue;
        }
        // only set specified tags
        let lc_tag = lc(&pl::call(it, "Image::ExifTool::GetTagName", vec![tag.clone()])?);
        let mut grp: Vec<Value> = Vec::new();
        let mut grp_h: std::collections::HashSet<Vec<u8>> = Default::default();
        'set: for set in &set_list {
            let opts2 = set.at(3);
            if opts2.get("EXPR").truthy() {
                continue; // (expressions handled in step 4)
            }
            if opts2.get("GROUP8").truthy() != is_alt {
                continue;
            }
            // check first for matching tag
            if !(eq(&set.at(1), &lc_tag) || pl::str_eq(&set.at(1), "*")) {
                // handle wildcards
                if !(opts2.get("WILD").truthy() && pl::re_dyn(&anchored(&set.at(1)), "")?.is_match(&lc_tag)) {
                    continue;
                }
            }
            // then check for matching group
            if !set.at(0).is_empty() {
                // get lower case group names if not done already
                if grp.is_empty() {
                    grp = pl::method_list(it, &src_exif_tool, "GetGroup", vec![tag.clone()])?.iter().map(lc).collect();
                    for g in &grp {
                        grp_h.insert(g.to_bytes().to_vec());
                    }
                }
                for fgv in set.at(0).items() {
                    let (f, g) = (fgv.at(0), fgv.at(1));
                    if f.is_undef() {
                        if !grp_h.contains(&g.to_bytes()[..]) {
                            continue 'set;
                        }
                    } else if pl::num_eq(&f, &n(7)) {
                        let id = pl::method(it, &src_exif_tool, "GetTagID", vec![tag.clone()])?;
                        if !pl::call(it, "Image::ExifTool::IsSameID", vec![id, g])?.truthy() {
                            continue 'set;
                        }
                    } else {
                        let gf = grp.get(f.to_i64() as usize).cloned().unwrap_or_default();
                        if !(!gf.is_undef() && eq(&g, &gf)) {
                            continue 'set;
                        }
                    }
                }
            }
            if !set.at(2).truthy() {
                break; // all done if we hit an exclude
            }
            // add to the list of tags matching this setTag
            set_matches.entry(set.to_bytes().to_vec()).or_default().push(tag.clone());
        }
    }
    // 4) loop through each setTag in original order, setting new tag values
    for set in set_list.iter().rev() {
        // get options for SetNewValue
        let opts2 = set.at(3);
        // handle expressions
        if opts2.get("EXPR").truthy() || opts2.get("ASSIGN").truthy() {
            let val;
            if opts2.get("EXPR").truthy() {
                val = pl::method(it, &src_exif_tool, "InsertTagValues", vec![set.at(1), Value::new_array(tags.clone()), s("Error")])?;
                let mut err = viv(&src_exif_tool, "VALUE").get("Error");
                if err.truthy() {
                    // pass on any error as a warning unless it is suppressed
                    let no_warn = viv(&src_exif_tool, "OPTIONS").get("NoWarning");
                    let matches = |e: &Value| -> bool {
                        match pl::re_dyn(&no_warn.to_bytes(), "") {
                            Ok(r) => r.is_match(e),
                            Err(_) => false,
                        }
                    };
                    let suppressed = no_warn.truthy()
                        && (matches(&err) || {
                            // (also apply expression to warning without "[minor] " prefix)
                            re(r"^\[minor\] ", "i").subst(&mut err, false, |_| Vec::new()) > 0 && matches(&err)
                        });
                    if !suppressed {
                        let tag = next_free(&rtn_info, "Warning");
                        rtn_info.set(tag.to_bytes(), viv(&src_exif_tool, "VALUE").get("Error"));
                    }
                    viv(&src_exif_tool, "VALUE").delete("Error");
                    if val.is_undef() {
                        continue;
                    }
                }
            } else {
                val = set.at(1);
            }
            let (dst_grp, dst_tag) = (set.at(2).at(0), set.at(2).at(1));
            if !(re(r"[?*]", "").is_match(&dst_tag) && !pl::str_eq(&dst_tag, "*")) {
                opts2.set("Protected", n(1));
            }
            if dst_grp.truthy() {
                opts2.set("Group", dst_grp);
            }
            let (r0, r1) = snv(it, &self_, dst_tag.clone(), val.clone(), &opts2)?;
            if r0.truthy() {
                rtn_info.set(dst_tag.to_bytes(), val); // tag was set successfully
            }
            // return warning if any
            if r1.truthy() {
                let k = next_free(&rtn_info, "Warning");
                rtn_info.set(k.to_bytes(), r1);
            }
            continue;
        }
        let matches = set_matches.get(&set.to_bytes()[..]).cloned().unwrap_or_default();
        for tag in matches {
            let mut no_warn = false;
            let val = if opts2.get("SrcType").truthy() && !eq(&opts2.get("SrcType"), &src_type) {
                pl::method(it, &src_exif_tool, "GetValue", vec![tag.clone(), opts2.get("SrcType")])?
            } else {
                info.get(tag.to_bytes())
            };
            let (dst_grp, mut dst_tag) = (set.at(2).at(0), set.at(2).at(1));
            if dst_grp.truthy() {
                let mut dst_grps = split_on(":", &dst_grp);
                // destination group of '*' writes to same group as source tag
                // (family 1 unless otherwise specified)
                for d in dst_grps.iter_mut() {
                    let Some(c) = re(r"^(\d*)(all|\*)$", "i").m(d) else { continue };
                    let fam = cap(&c, 1);
                    let fam_n = if length_of(&fam) > 0 { fam.clone() } else { n(1) };
                    let g = pl::method(it, &src_exif_tool, "GetGroup", vec![tag.clone(), fam_n])?;
                    *d = pl::concat(&fam, &g);
                    no_warn = true; // don't warn on wildcard destinations
                }
                opts2.set("Group", pl::join(":", &dst_grps));
            } else {
                opts2.delete("Group");
            }
            // transfer maker note information if setting this tag
            let ti = viv(&viv(&src_exif_tool, "TAG_INFO"), tag.to_bytes());
            if ti.get("MakerNotes").truthy() {
                opts2.set("Fixup", viv(&viv(&src_exif_tool, "TAG_EXTRA"), tag.to_bytes()).get("Fixup"));
                self_.set("MAKER_NOTE_BYTE_ORDER", src_exif_tool.get("MAKER_NOTE_BYTE_ORDER"));
            }
            if pl::str_eq(&dst_tag, "*") {
                dst_tag = tag.clone();
                no_warn = true;
            }
            if pl::str_eq(&set.at(1), "*") || set.at(3).get("WILD").truthy() {
                // don't copy from protected binary tags when using wildcards
                let ti = viv(&viv(&src_exif_tool, "TAG_INFO"), tag.to_bytes());
                if ti.get("Protected").truthy() && viv(&viv(&src_exif_tool, "TAG_INFO"), tag.to_bytes()).get("Binary").truthy() {
                    continue;
                }
                // don't copy to protected tags when using wildcards
                opts2.delete("Protected");
                // don't copy flattened tags if copying structures too when copying all
                opts2.set("NoFlat", if pl::str_eq(&struct_opt, "2") { n(1) } else { n(0) });
            } else {
                // allow protected tags to be copied if specified explicitly
                if !re(r"[?*]", "").is_match(&dst_tag) {
                    opts2.set("Protected", n(1));
                }
                opts2.delete("NoFlat");
            }
            // set value(s) for this tag
            let (rtn, wrn) = snv(it, &self_, dst_tag.clone(), val.clone(), &opts2)?;
            // this was added in version 9.14, and allowed actions like "-subject<all" to
            // write values of multiple tags into a list, but it had the side effect of
            // duplicating items if there were multiple list tags with the same name
            // (eg. -use mwg "-creator<creator"), so disable this as of ExifTool 9.36:
            // $$opts{Replace} = 0;    # accumulate values from tags matching a single argument
            if wrn.truthy() && !no_warn {
                // return this warning
                let k = next_free(&rtn_info, "Warning");
                rtn_info.set(k.to_bytes(), wrn);
            }
            opts2.delete("Fixup");
            if rtn.truthy() {
                rtn_info.set(tag.to_bytes(), val); // tag was set successfully
            }
        }
    }
    ret(rtn_info) // return information that we set
}

/// `$$h{key}++` (the old value; perl gives 0 for an undefined one).
fn post_inc_raw(h: &Value, key: &str) -> Value {
    post_inc(h, key)
}

/// `return ($numSet, $err) if wantarray; $err and warn "$err\n"; return $numSet;`
fn ret_set(it: &mut Interp, num_set: Value, err: Value, want: Want) -> ER<Vec<Value>> {
    if want == Want::List {
        return Ok(vec![num_set, err]);
    }
    if err.truthy() {
        warn_nl(it, &err)?;
    }
    ret(num_set)
}

/// Set tag value
/// Inputs: 0) ExifTool object reference
///         1) tag key, tag name, or '*' (optionally prefixed by group name),
///            or undef to reset all previous SetNewValue() calls
///         2) new value (scalar, scalar ref or list ref), or undef to delete tag
///         3-N) Options (see Writer.pl)
/// Returns: number of tags set (plus error string in list context)
pub fn set_new_value(it: &mut Interp, a: Vec<Value>, want: Want) -> ER<Vec<Value>> {
    pl::local_topic(it, |it| set_new_value_body(it, a, want))
}

fn set_new_value_body(it: &mut Interp, a: Vec<Value>, want: Want) -> ER<Vec<Value>> {
    let self_ = arg(&a, 0);
    let mut tag = arg(&a, 1);
    let mut value = arg(&a, 2);
    let options = Value::new_hash();
    let mut i = 3;
    while i < a.len() {
        options.set(a[i].to_bytes(), arg(&a, i + 1));
        i += 2;
    }
    let mut err = Value::Undef;
    let mut verbose = opt(&self_, "Verbose");
    let out = opt(&self_, "TextOut");
    let protected = if options.get("Protected").truthy() { options.get("Protected") } else { n(0) };
    let mut list_only = options.get("ListOnly");
    let set_tags = options.get("SetTags");
    let no_flat = options.get("NoFlat");
    let mut num_set = n(0);
    let gt = |v: &Value, x: i64| pl::gt(v, &n(x));

    if tag.is_undef() {
        self_.delete("NEW_VALUE");
        self_.set("NV_COUNT", n(0));
        self_.set("SAVE_COUNT", n(0));
        self_.set("DEL_GROUP", Value::new_hash());
        return ret(n(1));
    }
    // allow value to be scalar or list reference
    if value.is_ref() {
        if value.ref_type() == "ARRAY" {
            // value is an ARRAY so it may have more than one entry
            // - set values both separately and as a combined string if there are more than one
            if value.len() > 1 {
                // set all list-type tags first
                let replace = options.get("Replace");
                let mut no_join = false;
                for v in value.items() {
                    if v.is_ref() {
                        no_join = true;
                    }
                    let mut args = vec![self_.clone(), tag.clone(), v];
                    args.extend(hash_pairs(&options));
                    args.push(s("ListOnly"));
                    args.push(n(1));
                    let r = set_new_value(it, args, Want::List)?;
                    let (n_, e) = (arg(&r, 0), arg(&r, 1));
                    if e.truthy() {
                        err = e;
                    }
                    num_set = pl::add(&num_set, &n_);
                    options.delete("Replace"); // don't replace earlier values in list
                }
                if no_join {
                    return ret(num_set); // don't join if list contains objects
                }
                // and now set only non-list tags
                let sep = opt(&self_, "ListSep").to_bytes();
                value = Value::bytes(value.items().iter().map(|x| x.to_bytes().to_vec()).collect::<Vec<_>>().join(&sep[..]));
                options.set("Replace", replace);
                list_only = n(0);
                options.set("ListOnly", n(0));
            } else {
                value = value.at(0);
                if value.ref_type() == "SCALAR" {
                    value = value.deref(); // (handle single scalar ref in a list)
                }
            }
        } else if value.ref_type() == "SCALAR" {
            value = value.deref();
        }
    }
    // un-escape as necessary and make sure the Perl UTF-8 flag is OFF for the value
    // if perl is 5.6 or greater (otherwise our byte manipulations get corrupted!!)
    if !value.is_undef() && !value.is_ref() && !options.get("Sanitized").truthy() {
        let vr = pl::sref(value.clone());
        sanitize(it, vec![self_.clone(), vr.clone()], Want::Void)?;
        value = vr.deref();
    }

    // set group name in options if specified
    if let Some(c) = re(r"(.*):(.+)", "").m(&tag) {
        options.set("Group", cap(&c, 1));
        tag = cap(&c, 2);
    }

    // allow trailing '#' for ValueConv value
    if re(r"#$", "").subst(&mut tag, false, |_| Vec::new()) > 0 {
        options.set("Type", s("ValueConv"));
    }
    let conv_type = if options.get("Type").truthy() {
        options.get("Type")
    } else if opt(&self_, "PrintConv").truthy() {
        s("PrintConv")
    } else {
        s("ValueConv")
    };

    // filter value if necessary
    if pl::str_eq(&conv_type, "PrintConv") {
        let vr = pl::sref(value.clone());
        let ok = pl::method(it, &self_, "Filter", vec![opt(&self_, "FilterW"), vr.clone()])?;
        value = vr.deref();
        if !ok.truthy() {
            return ret(n(0));
        }
    }

    let mut want_groups: Vec<(Value, Value)> = Vec::new();
    let mut family2 = false;
    let mut want_group = options.get("Group");
    if want_group.truthy() {
        let family2groups = my_hash(it, "family2groups");
        for x in split_on(":", &want_group) {
            it.set_scalar("main::_", x.clone());
            if length_of(&x) == 0 {
                continue;
            }
            let Some(c) = re(r"^(\d+)?(.*)", "").m(&x) else { continue }; // separate family number and group name
            let (f, mut g) = (cap(&c, 1), cap(&c, 2));
            let lcg = lc(&g);
            // save group/family unless '*' or 'all'
            if !(pl::str_eq(&lcg, "*") || pl::str_eq(&lcg, "all")) {
                want_groups.push((f.clone(), lcg.clone()));
            }
            if re(r"^ID-", "i").subst(&mut g, false, |_| Vec::new()) > 0 {
                // family 7 is a tag ID
                if !f.is_undef() && !pl::str_eq(&f, "7") {
                    return ret(n(0));
                }
                if let Some(l) = want_groups.last_mut() {
                    *l = (n(7), g); // group name with 'ID-' removed and case preserved
                }
            } else if !f.is_undef() {
                if pl::gt(&f, &n(2)) {
                    return ret(n(0)); // only allow family 0, 1 or 2
                }
                if pl::num_eq(&f, &n(2)) {
                    family2 = true; // set flag indicating family 2 was used
                }
            } else if family2groups.get(lcg.to_bytes()).truthy() {
                family2 = true;
            }
        }
        if want_groups.is_empty() {
            want_group = Value::Undef;
        }
    }

    re(r" .*", "").subst(&mut tag, false, |_| Vec::new()); // convert from tag key to tag name if necessary
    if pl::str_eq(&lc(&tag), "all") {
        tag = s("*"); // use '*' instead of 'all'
    }
    //
    // handle group delete
    //
    if pl::str_eq(&tag, "*") && value.is_undef() && !family2 && want_groups.len() < 2 {
        'grpdel: {
            // set groups to delete
            let mut del: Vec<Value> = Vec::new();
            let remove = options.get("Replace").truthy() && gt(&options.get("Replace"), 1);
            let del_groups = my_array(it, "delGroups").items();
            if want_group.truthy() {
                if !re(r"^XM[LP]-\*$", "i").is_match(&want_group) {
                    del = grep_dyn(&anchored(&want_group), "i", &del_groups)?;
                }
                // remove associated groups when excluding from mass delete
                if !del.is_empty() && remove {
                    // remove associated groups in other family
                    let eg = my_hash(it, "excludeGroups").get(del[0].to_bytes());
                    if eg.truthy() {
                        del.extend(eg.items());
                    }
                    // remove upstream groups according to JPEG map
                    let jpeg_map = my_hash(it, "jpegMap");
                    let mut dir_name = del[0].clone();
                    let mut dir_names: Vec<Value> = Vec::new();
                    loop {
                        let mut parent = jpeg_map.get(dir_name.to_bytes());
                        if parent.is_ref() {
                            dir_names.extend(parent.items());
                            parent = dir_names.pop().unwrap_or_default();
                        }
                        dir_name = if parent.truthy() {
                            parent
                        } else if !dir_names.is_empty() {
                            dir_names.remove(0)
                        } else {
                            Value::Undef
                        };
                        if !dir_name.truthy() {
                            break;
                        }
                        del.push(dir_name.clone()); // exclude this too
                    }
                }
                // allow MIE groups to be deleted by number,
                // and allow any XMP family 1 group to be deleted
                if re(r"^(MIE\d+|XM[LP]-[-\w]*\w)$", "i").is_match(&want_group) {
                    del.push(pl::uc(&want_group));
                }
            } else {
                // push all groups plus '*', except the protected groups
                let pg = my_scalar(it, "protectedGroups").borrow().clone();
                let r = pl::re_dyn(&anchored(&pg), "")?;
                del.extend(del_groups.iter().filter(|g| !r.is_match(g)).cloned());
                del.push(s("*"));
            }
            if !del.is_empty() {
                num_set = pl::add(&num_set, &n(1));
                let mut donegrps: Vec<Value> = Vec::new();
                let del_group = self_.get("DEL_GROUP");
                let del_more = my_hash(it, "delMore");
                for grp in del {
                    if remove {
                        let mut did_excl = false;
                        if let Some(c) = re(r"^(XM[LP])(-.*)?$", "").m(&grp) {
                            let x = cap(&c, 1);
                            if eq(&grp, &x) {
                                // exclude all related family 1 groups too
                                let mut p = b"^(-?)".to_vec();
                                p.extend_from_slice(&x.to_bytes());
                                p.push(b'-');
                                let r = pl::re_dyn(&p, "")?;
                                for k in keys(&del_group) {
                                    let Some(m) = r.m(&k) else { continue };
                                    if !cap(&m, 1).truthy() {
                                        donegrps.push(k.clone());
                                    }
                                    del_group.delete(k.to_bytes());
                                }
                            } else if del_group.get(cat(&[&x, &s("-*")]).to_bytes()).truthy() && !del_group.get(cat(&[&s("-"), &grp]).to_bytes()).truthy() {
                                // must also exclude XMP or XML to prevent bulk delete
                                if del_group.get(x.to_bytes()).truthy() {
                                    donegrps.push(x.clone());
                                    del_group.delete(x.to_bytes());
                                }
                                // flag XMP/XML family 1 group for exclusion with leading '-'
                                del_group.set(cat(&[&s("-"), &grp]).to_bytes(), n(1));
                                did_excl = true;
                            }
                        }
                        if del_group.exists(grp.to_bytes()) {
                            del_group.delete(grp.to_bytes());
                        } else if !did_excl {
                            continue;
                        }
                    } else {
                        del_group.set(grp.to_bytes(), n(1));
                        // add extra groups to delete if necessary
                        let dm = del_more.get(grp.to_bytes());
                        if dm.truthy() {
                            for x in dm.items() {
                                del_group.set(x.to_bytes(), n(1));
                                donegrps.push(x);
                            }
                        }
                        // remove all of this group from previous new values
                        remove_new_values_for_group(it, vec![self_.clone(), grp.clone()], Want::Void)?;
                    }
                    donegrps.push(grp);
                }
                if gt(&verbose, 1) && !donegrps.is_empty() {
                    let donegrps = sort_str(donegrps);
                    let msg = if remove { "Excluding from deletion" } else { "Deleting tags in" };
                    print_out(it, &out, cat(&[&s(&format!("  {msg}: ")), &pl::join(" ", &donegrps), &s("\n")]))?;
                }
            } else if !grep_dyn(&anchored(&want_group), "i", &my_array(it, "delGroup2").items())?.is_empty() {
                break 'grpdel; // allow tags to be deleted by group2 name
            } else {
                err = pl::concat(&s("Not a deletable group: "), &want_group);
            }
            // all done
            return ret_set(it, num_set, err, want);
        }
    }

    // initialize write/create flags
    let mut create_only = false;
    let mut edit_only = options.get("EditOnly");
    let mut edit_group = options.get("EditGroup");
    let write_mode = opt(&self_, "WriteMode");
    if !pl::str_eq(&write_mode, "wcg") {
        if !re(r"w", "i").is_match(&write_mode) {
            create_only = true; // don't write existing tags
        }
        if !re(r"c", "i").is_match(&write_mode) {
            if create_only {
                return ret(n(0)); // nothing to do unless writing existing tags
            }
            edit_only = n(1); // don't create new tags
        } else if !re(r"g", "i").is_match(&write_mode) {
            edit_group = n(1); // don't create new groups
        }
    }
    let (mut ifd_name, mut mie_group, mut mov_group) = (Value::Undef, Value::Undef, Value::Undef);
    // set family 1 group names
    let exif_dirs = my_hash(it, "exifDirs");
    let all_fam0 = my_hash(it, "allFam0");
    for (f, g) in want_groups.clone() {
        if !f.is_undef() && !pl::num_eq(&f, &n(1)) {
            continue;
        }
        let x = g.clone();
        it.set_scalar("main::_", x.clone());
        // set $ifdName if this group is a valid IFD or SubIFD name
        let mut grp_name = Value::Undef;
        if let Some(c) = re(r"^IFD(\d+)$", "i").m(&x) {
            grp_name = pl::concat(&s("IFD"), &cap(&c, 1));
            ifd_name = grp_name.clone();
        } else if let Some(c) = re(r"^SubIFD(\d+)$", "i").m(&x) {
            grp_name = pl::concat(&s("SubIFD"), &cap(&c, 1));
            ifd_name = grp_name.clone();
        } else if let Some(c) = re(r"^Version(\d+)$", "i").m(&x) {
            grp_name = pl::concat(&s("Version"), &cap(&c, 1)); // Sony IDC VersionIFD
            ifd_name = grp_name.clone();
        } else if exif_dirs.get(x.to_bytes()).truthy() {
            grp_name = exif_dirs.get(x.to_bytes());
            if !(ifd_name.truthy() && all_fam0.get(x.to_bytes()).truthy()) {
                ifd_name = grp_name.clone();
            }
        } else if all_fam0.get(x.to_bytes()).truthy() {
            grp_name = all_fam0.get(x.to_bytes());
        } else if let Some(c) = re(r"^Track(\d+)$", "i").m(&x) {
            grp_name = pl::concat(&s("Track"), &cap(&c, 1)); // QuickTime track
            mov_group = grp_name.clone();
        } else if let Some(c) = re(r"^MIE(\d*-?)(\w+)$", "i").m(&x) {
            grp_name = cat(&[&s("MIE"), &cap(&c, 1), &pl::ucfirst(&lc(&cap(&c, 2)))]);
            mie_group = grp_name.clone();
        } else if !ifd_name.truthy() && re(r"^XMP\b", "i").is_match(&x) {
            // must load XMP table to set group1 names
            let table = pl::call(it, "Image::ExifTool::GetTagTable", vec![s("Image::ExifTool::XMP::Main")])?;
            let write_proc = table.get("WRITE_PROC");
            if write_proc.truthy() {
                pl::call_code(it, &write_proc, vec![], Want::Scalar)?;
            }
        }
        // fix case for known groups
        if grp_name.truthy() && !eq(&grp_name, &x) {
            let gb = grp_name.to_bytes().to_vec();
            pl::re_dyn(&gb, "i")?.subst(&mut want_group, false, |_| gb.clone());
        }
    }
    //
    // get list of tags we want to set
    //
    let orig_tag = tag.clone();
    let mut matching_tags = pl::call_list(it, "Image::ExifTool::TagLookup::FindTagInfo", vec![tag.clone()])?;
    if matching_tags.is_empty() {
        let mut lang_code = Value::Undef;
        // allow language suffix of form "-en_CA" or "-<rfc3066>" on tag name
        let m = re(r"^([?*\w]+)-([a-z]{2})(_[a-z]{2})$", "i").m(&tag).or_else(|| re(r"^([?*\w]+)-([a-z]{2,3}|[xi])(-[a-z\d]{2,8}(-[a-z\d]{1,8})*)?$", "i").m(&tag)); // MIE / XMP/PNG/QuickTime
        let mut found = false;
        if let Some(c) = m {
            tag = cap(&c, 1);
            // normalize case of language codes
            lang_code = lc(&cap(&c, 2));
            let c3 = cap(&c, 3);
            if c3.truthy() {
                lang_code = pl::concat(&lang_code, &(if length_of(&c3) == 3 { pl::uc(&c3) } else { lc(&c3) }));
            }
            let new_matches = pl::call_list(it, "Image::ExifTool::TagLookup::FindTagInfo", vec![tag.clone()])?;
            for tag_info in new_matches {
                // only allow language codes in tables which support them
                if !tag_info.get("Table").truthy() {
                    continue;
                }
                let lang_info_proc = viv(&tag_info, "Table").get("LANG_INFO");
                if !lang_info_proc.truthy() {
                    continue;
                }
                let lang_info = pl::call_code(it, &lang_info_proc, vec![tag_info.clone(), lang_code.clone()], Want::Scalar)?.pop().unwrap_or_default();
                if lang_info.truthy() {
                    matching_tags.push(lang_info);
                }
            }
            found = !matching_tags.is_empty();
        } else if !options.get("NoShortcut").truthy() {
            // look for a shortcut or alias
            crate::runtime::require(it, "Image::ExifTool::Shortcuts")?;
            let main = pl::pkg_hash(it, "Image::ExifTool::Shortcuts::Main");
            let mut p = b"^".to_vec();
            p.extend_from_slice(&crate::perl::regex::quotemeta(&tag.to_bytes()));
            p.push(b'$');
            let match_ = grep_dyn(&p, "i", &keys(&main))?.into_iter().next();
            err = Value::Undef;
            if let Some(match_) = match_ {
                options.set("NoShortcut", n(1));
                options.set("Sanitized", n(1));
                for t in main.get(match_.to_bytes()).items() {
                    let (n_, e) = snv(it, &self_, t, value.clone(), &options)?;
                    num_set = pl::add(&num_set, &n_);
                    if e.truthy() {
                        err = e;
                    }
                }
                if num_set.truthy() {
                    err = Value::Undef; // no error if any set successfully
                }
                return ret_set(it, num_set, err, want);
            }
        }
        if !found {
            if !list_only.truthy() {
                if !pl::call(it, "Image::ExifTool::TagLookup::TagExists", vec![tag.clone()])?.truthy() {
                    if re(r"^[-\w*?]+$", "").is_match(&tag) {
                        let pre = if want_group.truthy() { pl::concat(&want_group, &s(":")) } else { s("") };
                        err = cat(&[&s("Tag '"), &pre, &orig_tag, &s("' is not defined")]);
                        if re(r"-", "").is_match(&orig_tag) {
                            err = pl::concat(&err, &s(" or has a bad language code"));
                        }
                        if !pre.truthy() && pl::str_eq(&pl::uc(&orig_tag), "TAG") {
                            err = cat(&[&err, &s(" (specify a writable tag name, not '"), &orig_tag, &s("' literally)")]);
                        }
                    } else {
                        err = cat(&[&s("Invalid tag name '"), &tag, &s("'")]);
                        if re(r"^\$", "").is_match(&tag) {
                            err = pl::concat(&err, &s(" (remove the leading '$')"));
                        }
                    }
                } else if lang_code.truthy() {
                    err = cat(&[&s("Tag '"), &tag, &s("' does not support alternate languages")]);
                } else if want_group.truthy() {
                    err = cat(&[&s("Sorry, "), &want_group, &s(":"), &orig_tag, &s(" doesn't exist or isn't writable")]);
                } else {
                    err = cat(&[&s("Sorry, "), &orig_tag, &s(" is not writable")]);
                }
                if gt(&verbose, 2) {
                    print_out(it, &out, pl::concat(&err, &s("\n")))?;
                }
            }
            // all done
            return ret_set(it, num_set, err, want);
        }
    }
    // get group name that we're looking for
    let mut found_match = 0;
    //
    // determine the groups for all tags found, and the tag with
    // the highest priority group
    //
    let mut tag_info_list: Vec<Value> = Vec::new();
    let mut write_also_list: Vec<Value> = Vec::new();
    let write_group_h = Value::new_hash();
    let preferred = Value::new_hash();
    let tag_priority = Value::new_hash();
    let avoid = Value::new_hash();
    let highest_priority = Value::new_hash();
    let highest_qt = Value::new_hash();
    let mut was_protected = Value::Undef;
    let mut no_create = Value::Undef;
    let translate_want_group = my_hash(it, "translateWantGroup");
    let translate_write_group = my_hash(it, "translateWriteGroup");

    'tag: for tag_info in matching_tags.clone() {
        let tik = tag_info.to_bytes();
        tag = tag_info.get("Name"); // get tag name for warnings
        let lc_tag = lc(&tag); // get lower-case tag name for use in variables
        let lk = lc_tag.to_bytes();
        // initialize highest priority if we are starting a new tag
        if highest_priority.get(&lk).is_undef() {
            highest_priority.set(&lk, n(-999));
        }
        let mut priority = Value::Undef;
        let mut write_group = Value::Undef;
        let prf_tag = if !tag_info.get("Preferred").is_undef() { tag_info.get("Preferred") } else { viv(&tag_info, "Table").get("PREFERRED") };
        if want_group.truthy() {
            // a WriteGroup of All is special
            let wg_all = tag_info.get("WriteGroup").truthy() && pl::str_eq(&tag_info.get("WriteGroup"), "All");
            let grp = pl::method_list(it, &self_, "GetGroup", vec![tag_info.clone()])?;
            let g = |i: usize| grp.get(i).cloned().unwrap_or_default();
            let mut hi_pri = n(1000);
            for (fam, lc_want0) in want_groups.clone() {
                let mut lc_want = lc_want0;
                let tw = translate_want_group.get(lc_want.to_bytes());
                if tw.truthy() {
                    lc_want = tw;
                }
                // only set tag in specified group
                // bump priority of preferred tag
                if prf_tag.truthy() {
                    hi_pri = pl::add(&hi_pri, &prf_tag);
                }
                if fam.is_undef() {
                    if eq(&lc_want, &lc(&g(0))) {
                        // don't go to more general write group of "All"
                        // if something more specific was wanted
                        if wg_all && !write_group.truthy() {
                            write_group = g(0);
                        }
                        continue;
                    }
                    if eq(&lc_want, &lc(&g(2))) {
                        continue;
                    }
                } else if pl::num_eq(&fam, &n(7)) {
                    if pl::call(it, "Image::ExifTool::IsSameID", vec![tag_info.get("TagID"), lc_want.clone()])?.truthy() {
                        continue;
                    }
                } else if !pl::num_eq(&fam, &n(1)) && !tag_info.get("AllowGroup").truthy() {
                    if eq(&lc_want, &lc(&g(fam.to_i64() as usize))) {
                        continue;
                    }
                    if wg_all && !fam.truthy() && all_fam0.get(lc_want.to_bytes()).truthy() {
                        if !write_group.truthy() {
                            write_group = all_fam0.get(lc_want.to_bytes());
                        }
                        continue;
                    }
                    continue 'tag; // wrong group
                }
                // handle family 1 groups specially
                if pl::str_eq(&g(0), "EXIF") || pl::str_eq(&g(0), "SonyIDC") || wg_all {
                    if !(ifd_name.truthy() && eq(&lc_want, &lc(&ifd_name))) {
                        if !(wg_all && !fam.truthy() && all_fam0.get(lc_want.to_bytes()).truthy()) {
                            continue 'tag;
                        }
                        if !write_group.truthy() {
                            write_group = all_fam0.get(lc_want.to_bytes());
                        }
                        continue;
                    }
                    if wg_all && all_fam0.get(lc_want.to_bytes()).truthy() && fam.truthy() {
                        continue 'tag;
                    }
                    // can't yet write PreviewIFD tags (except for image)
                    if pl::str_eq(&lc_want, "PreviewIFD") {
                        found_match += 1;
                        continue 'tag;
                    }
                    write_group = ifd_name.clone(); // write to the specified IFD
                } else if pl::str_eq(&g(0), "QuickTime") {
                    if pl::str_eq(&g(1), "Track#") {
                        if !(mov_group.truthy() && eq(&lc_want, &lc(&mov_group))) {
                            continue 'tag;
                        }
                        write_group = mov_group.clone();
                    } else {
                        let wg = viv(&tag_info, "Table").get("WRITE_GROUP");
                        if !(wg.truthy() && eq(&lc_want, &lc(&wg))) {
                            continue 'tag;
                        }
                        write_group = wg;
                    }
                } else if pl::str_eq(&g(0), "MIE") {
                    if !(mie_group.truthy() && eq(&lc_want, &lc(&mie_group))) {
                        continue 'tag;
                    }
                    write_group = mie_group.clone(); // write to specific MIE group
                    // set specific write group with document number if specified
                    let twg = viv(&tag_info, "Table").get("WRITE_GROUP");
                    if re(r"^MIE\d+$", "").is_match(&write_group) && twg.truthy() {
                        write_group = twg;
                        let mg = mie_group.to_bytes().to_vec();
                        re(r"^MIE", "").subst(&mut write_group, false, |_| mg.clone());
                    }
                } else if !tag_info.get("AllowGroup").truthy() || !pl::re_dyn(&anchored(&tag_info.get("AllowGroup")), "i")?.is_match(&lc_want) {
                    // allow group1 name to be specified
                    if !eq(&lc_want, &lc(&g(1))) {
                        continue 'tag;
                    }
                }
            }
            if !write_group.truthy() {
                write_group = tag_info.get("WriteGroup");
                if !write_group.truthy() {
                    write_group = viv(&tag_info, "Table").get("WRITE_GROUP");
                }
                if !write_group.truthy() {
                    write_group = g(0);
                }
            }
            priority = hi_pri; // highest priority since group was specified
        }
        found_match += 1;
        // must do a dummy call to the write proc to autoload write package
        // before checking Writable flag
        let table = tag_info.get("Table");
        let mut write_proc = table.get("WRITE_PROC");
        // load source table if this was a user-defined table
        if table.get("SRC_TABLE").truthy() {
            let src = pl::call(it, "Image::ExifTool::GetTagTable", vec![table.get("SRC_TABLE")])?;
            if !write_proc.truthy() {
                write_proc = src.get("WRITE_PROC");
            }
        }
        if write_proc.truthy() {
            // make sure module is loaded if the writeProc is a string
            if !write_proc.is_ref() {
                let mut module = write_proc.clone();
                if re(r"::\w+$", "").subst(&mut module, false, |_| Vec::new()) > 0 {
                    pl::eval_block(it, |it| perl_snippet(it, "eval \"require $module\"", &[("module", module.clone())], Want::Scalar))?;
                }
            }
            if !(write_proc.truthy() && pl::call_code(it, &write_proc, vec![], Want::Scalar)?.pop().unwrap_or_default().truthy()) {
                continue;
            }
        }
        // must still check writable flags in case of UserDefined tags
        let writable = tag_info.get("Writable");
        if !(writable.truthy() || (table.get("WRITABLE").truthy() && writable.is_undef() && !tag_info.get("SubDirectory").truthy())) {
            continue;
        }
        // set specific write group (if we didn't already)
        if !write_group.truthy() || (translate_write_group.get(write_group.to_bytes()).truthy() && (!tag_info.get("WriteGroup").truthy() || !pl::str_eq(&tag_info.get("WriteGroup"), "All"))) {
            // use default write group
            write_group = tag_info.get("WriteGroup");
            if !write_group.truthy() {
                write_group = viv(&tag_info, "Table").get("WRITE_GROUP");
            }
            // use group 0 name if no WriteGroup specified
            let group0 = pl::method(it, &self_, "GetGroup", vec![tag_info.clone(), n(0)])?;
            if !write_group.truthy() {
                write_group = group0.clone();
            }
            // get priority for this group
            if !priority.truthy() {
                if tag_info.get("Avoid").truthy() && tag_info.get("WriteAlso").truthy() {
                    priority = n(0);
                } else {
                    let wp = viv(&self_, "WRITE_PRIORITY");
                    priority = wp.get(lc(&write_group).to_bytes());
                    if !priority.truthy() {
                        priority = wp.get(lc(&group0).to_bytes());
                        if !priority.truthy() {
                            priority = n(0);
                        }
                    }
                }
            }
            // adjust priority based on Preferred level for this tag
            if prf_tag.truthy() {
                priority = pl::add(&priority, &prf_tag);
            }
        }
        // don't write tag if protected
        let mut prot = tag_info.get("Protected");
        if no_flat.truthy() && !tag_info.get("Flat").is_undef() {
            prot = n(1);
        }
        if prot.truthy() {
            prot = ops::bit_and(&prot, &ops::bit_not(&protected));
            if prot.truthy() {
                was_protected = match prot.to_i64() {
                    1 => s("unsafe"),
                    2 => s("protected"),
                    3 => s("unsafe and protected"),
                    _ => Value::Undef,
                };
                if gt(&verbose, 1) {
                    let wgrp1 = write_group1(it, &self_, &tag_info, &write_group)?;
                    print_out(it, &out, cat(&[&s("Sorry, "), &wgrp1, &s(":"), &tag, &s(" is "), &was_protected, &s(" for writing\n")]))?;
                }
                continue;
            }
        }
        // set priority for this tag
        tag_priority.set(&tik, priority.clone());
        // keep track of highest priority QuickTime tag
        if pl::str_eq(&viv(&table, "GROUPS").get("0"), "QuickTime") && (highest_qt.get(&lk).is_undef() || pl::lt(&highest_qt.get(&lk), &priority)) {
            highest_qt.set(&lk, priority.clone());
        }
        if pl::gt(&priority, &highest_priority.get(&lk)) {
            highest_priority.set(&lk, priority.clone());
            let p = Value::new_hash();
            p.set(&tik, n(1));
            preferred.set(&lk, p);
            avoid.set(&lk, if tag_info.get("Avoid").truthy() { n(1) } else { n(0) });
        } else if pl::num_eq(&priority, &highest_priority.get(&lk)) {
            // create all tags with highest priority
            viv(&preferred, &lk).set(&tik, n(1));
            if tag_info.get("Avoid").truthy() {
                avoid.set(&lk, pl::add(&avoid.get(&lk), &n(1)));
            }
        }
        if tag_info.get("WriteAlso").truthy() {
            // store WriteAlso tags separately so we can set them first
            write_also_list.push(tag_info.clone());
        } else {
            tag_info_list.push(tag_info.clone());
        }
        // special case to allow override of XMP WriteGroup
        if pl::str_eq(&write_group, "XMP") {
            let mut wg = tag_info.get("WriteGroup");
            if !wg.truthy() {
                wg = table.get("WRITE_GROUP");
            }
            if wg.truthy() {
                write_group = wg;
            }
        }
        write_group_h.set(&tik, write_group);
    }
    // sort tag info list in reverse order of priority (highest number last)
    // so we get the highest priority error message in the end
    tag_info_list.sort_by(|x, y| ncmp(&tag_priority.get(x.to_bytes()), &tag_priority.get(y.to_bytes())));
    // must write any tags which also write other tags first
    if !write_also_list.is_empty() {
        write_also_list.extend(tag_info_list);
        tag_info_list = write_also_list;
    }

    // check priorities for each set of tags we are writing
    for lc_tag in keys(&preferred) {
        let lk = lc_tag.to_bytes();
        // don't create tags with priority 0 if group priorities are set
        if preferred.get(&lk).truthy() && pl::num_eq(&highest_priority.get(&lk), &n(0)) && !hash_rv(&self_.get("WRITE_PRIORITY"))?.is_empty() {
            preferred.delete(&lk);
        }
        // avoid creating tags with 'Avoid' flag set if there are other alternatives
        if avoid.get(&lk).truthy() && preferred.get(&lk).truthy() {
            if pl::lt(&avoid.get(&lk), &n(preferred.get(&lk).len() as i64)) {
                // just remove the 'Avoid' tags since there are other preferred tags
                for tag_info in &tag_info_list {
                    if !eq(&lc_tag, &lc(&tag_info.get("Name"))) {
                        continue;
                    }
                    if tag_info.get("Avoid").truthy() {
                        preferred.get(&lk).delete(tag_info.to_bytes());
                    }
                }
            } else if pl::lt(&highest_priority.get(&lk), &n(1000)) {
                // look for another priority tag to create instead
                let mut next_highest = n(0);
                let mut next_best_tags: Vec<Value> = Vec::new();
                for tag_info in &tag_info_list {
                    if !eq(&lc_tag, &lc(&tag_info.get("Name"))) {
                        continue;
                    }
                    let priority = tag_priority.get(tag_info.to_bytes());
                    if !priority.truthy() {
                        continue;
                    }
                    if pl::num_eq(&priority, &highest_priority.get(&lk)) {
                        continue;
                    }
                    if pl::lt(&priority, &next_highest) {
                        continue;
                    }
                    let mut permanent = tag_info.get("Permanent");
                    if permanent.is_undef() {
                        permanent = viv(tag_info, "Table").get("PERMANENT");
                    }
                    if tag_info.get("Avoid").truthy() || permanent.truthy() {
                        continue;
                    }
                    if pl::str_eq(&write_group_h.get(tag_info.to_bytes()), "MakerNotes") {
                        continue;
                    }
                    if pl::lt(&next_highest, &priority) {
                        next_highest = priority;
                        next_best_tags.clear();
                    }
                    next_best_tags.push(tag_info.clone());
                }
                if !next_best_tags.is_empty() {
                    // change our preferred tags to the next best tags
                    preferred.delete(&lk);
                    for tag_info in next_best_tags {
                        viv(&preferred, &lk).set(tag_info.to_bytes(), n(1));
                    }
                }
            }
        }
    }
    //
    // generate new value hash for each tag
    //
    let mut priority_set = false;
    let mut create_groups = Value::Undef;
    let also_wrote = Value::new_hash();

    self_.delete("CHECK_WARN"); // reset CHECK_PROC warnings

    // loop through all valid tags to find the one(s) to write
    'tl: for tag_info in &tag_info_list {
        let tik = tag_info.to_bytes();
        if also_wrote.get(&tik).truthy() {
            continue; // don't rewrite tags we already wrote
        }
        // only process List or non-List tags if specified
        if !list_only.is_undef() && (list_only.truthy() != tag_info.get("List").truthy()) {
            continue;
        }
        let mut no_conv = false;
        let write_group = write_group_h.get(&tik);
        let mut permanent = tag_info.get("Permanent");
        if permanent.is_undef() {
            permanent = viv(tag_info, "Table").get("PERMANENT");
        }
        if permanent.is_undef() && pl::str_eq(&write_group, "MakerNotes") {
            permanent = n(1);
        }
        let wgrp1 = write_group1(it, &self_, tag_info, &write_group)?;
        tag = tag_info.get("Name"); // get tag name for warnings
        let lc_tag = lc(&tag);
        let mut pref = preferred.get(lc_tag.to_bytes());
        if !pref.truthy() {
            pref = Value::new_hash();
        }
        // don't write Avoid-ed tags with side effect unless preferred
        if !pref.get(&tik).truthy() && tag_info.get("Avoid").truthy() && tag_info.get("WriteAlso").truthy() {
            continue;
        }
        let mut shift = options.get("Shift");
        let mut add_value = options.get("AddValue");
        let wt = cat(&[&wgrp1, &s(":"), &tag]);
        let mut val;
        // ('WriteAlso:' label: the steps below are skipped when the tag goes there directly)
        'write_also: {
            if !shift.is_undef() {
                // (can't currently shift list-type tags)
                let shiftable;
                if tag_info.get("List").truthy() {
                    shiftable = s(""); // can add/delete but not shift
                } else {
                    shiftable = tag_info.get("Shift");
                    if !shift.truthy() {
                        // set shift according to AddValue/DelValue
                        if add_value.truthy() {
                            shift = n(1);
                        }
                        // can shift a date/time with -=, but this is
                        // a conditional delete operation for other tags
                        if options.get("DelValue").truthy() && !shiftable.is_undef() && pl::str_eq(&shiftable, "Time") {
                            shift = n(-1);
                        }
                    }
                    if shift.truthy() && (value.is_undef() || length_of(&value) == 0) {
                        // (now allow -= to be used for shiftable tag - v8.05)
                        shift = Value::Undef;
                    }
                }
                // can't shift List-type tag
                if (!shiftable.is_undef() && !shiftable.truthy())
                    // and don't try to conditionally delete if Shift is "0"
                    && (shift.truthy() || (pl::str_eq(&shiftable, "0") && options.get("DelValue").truthy()))
                {
                    err = pl::concat(&wt, &s(" is not shiftable"));
                    if verbose.truthy() {
                        print_out(it, &out, pl::concat(&err, &s("\n")))?;
                    }
                    continue 'tl;
                }
            }
            val = value.clone();
            if !val.is_undef() {
                // check to make sure this is a List or Shift tag if adding
                if add_value.truthy() && !(shift.truthy() || tag_info.get("List").truthy()) {
                    if pl::str_eq(&add_value, "2") {
                        add_value = Value::Undef; // quietly reset this option
                    } else {
                        err = cat(&[&s("Can't add "), &wt, &s(" (not a List type)")]);
                        if gt(&verbose, 2) {
                            print_out(it, &out, pl::concat(&err, &s("\n")))?;
                        }
                        continue 'tl;
                    }
                }
                if shift.truthy() {
                    if tag_info.get("Shift").truthy() && pl::str_eq(&tag_info.get("Shift"), "Time") {
                        // add '+' or '-' prefix to indicate shift direction
                        val = pl::concat(&s(if pl::gt(&shift, &n(0)) { "+" } else { "-" }), &val);
                        // check the shift for validity
                        crate::runtime::require(it, "Image/ExifTool/Shift.pl")?;
                        let err2 = pl::call(it, "Image::ExifTool::CheckShift", vec![tag_info.get("Shift"), val.clone()])?;
                        if err2.truthy() {
                            err = cat(&[&err2, &s(" for "), &wt]);
                            if gt(&verbose, 2) {
                                print_out(it, &out, pl::concat(&err, &s("\n")))?;
                            }
                            continue 'tl;
                        }
                    } else if pl::call(it, "Image::ExifTool::IsFloat", vec![val.clone()])?.truthy() {
                        val = pl::mul(&val, &shift);
                    } else {
                        err = cat(&[&s("Shift value for "), &wt, &s(" is not a number")]);
                        if gt(&verbose, 2) {
                            print_out(it, &out, pl::concat(&err, &s("\n")))?;
                        }
                        continue 'tl;
                    }
                    no_conv = true; // no conversions if shifting tag
                } else if length_of(&val) == 0 && options.get("DelValue").truthy() {
                    no_conv = true; // no conversions for deleting empty value
                } else if val.ref_type() == "HASH" && !tag_info.get("Struct").truthy() {
                    err = cat(&[&s("Can't write a structure to "), &wt]);
                    if gt(&verbose, 2) {
                        print_out(it, &out, pl::concat(&err, &s("\n")))?;
                    }
                    continue 'tl;
                }
            } else if permanent.truthy() {
                if options.get("IgnorePermanent").truthy() {
                    return ret(n(0));
                }
                // can't delete permanent tags, so set them to DelValue or empty string instead
                if !tag_info.get("DelValue").is_undef() {
                    val = tag_info.get("DelValue");
                    no_conv = true; // DelValue is the raw value, so no conversion necessary
                } else {
                    val = s("");
                }
            } else if add_value.truthy() || options.get("DelValue").truthy() {
                err = cat(&[&s("No value to add or delete in "), &wt]);
                if gt(&verbose, 2) {
                    print_out(it, &out, pl::concat(&err, &s("\n")))?;
                }
                continue 'tl;
            } else {
                let del_check = tag_info.get("DelCheck");
                if del_check.truthy() {
                    //### eval DelCheck ($self, $tagInfo, $wantGroup)
                    let (mut sv, mut ti, mut wg, mut tg, mut v, mut vl) = (self_.clone(), tag_info.clone(), want_group.clone(), tag.clone(), val.clone(), value.clone());
                    let mut err2 = eval_vars(it, &del_check, &mut [("self", &mut sv), ("tagInfo", &mut ti), ("wantGroup", &mut wg), ("tag", &mut tg), ("val", &mut v), ("value", &mut vl)], |_| {}, Want::Scalar)?;
                    let e = eval_error(it);
                    if e.truthy() {
                        it.warn(e)?;
                        err2 = s("Error evaluating DelCheck");
                    }
                    if !err2.is_undef() {
                        // (allow other tags to be set using DelCheck as a hook)
                        if !err2.truthy() {
                            break 'write_also; // GOTO!
                        }
                        if !re(r"delete$", "").is_match(&err2) {
                            err2 = pl::concat(&err2, &s(" for"));
                        }
                        err = cat(&[&err2, &s(" "), &wt]);
                        if gt(&verbose, 2) {
                            print_out(it, &out, pl::concat(&err, &s("\n")))?;
                        }
                        continue 'tl;
                    }
                }
                // set group delete flag if this tag represents an entire group
                if tag_info.get("DelGroup").truthy() && !options.get("DelValue").truthy() {
                    let mut del = vec![tag.clone()];
                    viv(&self_, "DEL_GROUP").set(tag.to_bytes(), n(1));
                    // delete extra groups if necessary
                    let dm = my_hash(it, "delMore").get(tag.to_bytes());
                    if dm.truthy() {
                        for x in dm.items() {
                            viv(&self_, "DEL_GROUP").set(x.to_bytes(), n(1));
                            del.push(x);
                        }
                    }
                    // remove all of this group from previous new values
                    remove_new_values_for_group(it, vec![self_.clone(), tag.clone()], Want::Void)?;
                    if verbose.truthy() {
                        print_out(it, &out, cat(&[&s("  Deleting tags in: "), &pl::join(" ", &del), &s("\n")]))?;
                    }
                    num_set = pl::add(&num_set, &n(1));
                    continue 'tl;
                }
                no_conv = true; // value is not defined, so don't do conversion
            }
            // apply inverse PrintConv and ValueConv conversions
            // save ValueConv setting for use in ConvInv()
            if !no_conv {
                // set default conversion type used by ConvInv() and CHECK_PROC routines
                self_.set("ConvType", conv_type.clone());
                let r = conv_inv(it, vec![self_.clone(), val.clone(), tag_info.clone(), tag.clone(), wgrp1.clone(), self_.get("ConvType"), want_group.clone()], Want::List)?;
                val = arg(&r, 0);
                let e = arg(&r, 1);
                if !e.is_undef() {
                    // empty error string causes error to be ignored without setting the value
                    if !e.truthy() {
                        break 'write_also; // GOTO!
                    }
                    err = e;
                }
            }
            if val.is_undef() && !value.is_undef() {
                // if value conversion failed, we must still add a NEW_VALUE
                // entry for this tag it it was a DelValue
                if !options.get("DelValue").truthy() {
                    continue 'tl;
                }
                val = s("xxx never delete xxx");
            }
            if !self_.get("NEW_VALUE").truthy() {
                self_.set("NEW_VALUE", Value::new_hash());
            }
            if options.get("Replace").truthy() {
                // delete the previous new value
                gnvh(it, vec![self_.clone(), tag_info.clone(), write_group.clone(), s("delete"), options.get("ProtectSaved")])?;
                // also delete related tag previous new values
                if tag_info.get("WriteAlso").truthy() {
                    self_.set("INDENT2", s("+"));
                    let wgrp = if tag_info.get("WriteGroup").truthy() && pl::str_eq(&tag_info.get("WriteGroup"), "All") && write_group.truthy() { pl::concat(&write_group, &s(":")) } else { s("") };
                    for wtag in sort_str(keys(&tag_info.get("WriteAlso"))) {
                        let r = set_new_value(it, vec![self_.clone(), pl::concat(&wgrp, &wtag), Value::Undef, s("Replace"), n(2)], Want::List)?;
                        num_set = pl::add(&num_set, &arg(&r, 0));
                    }
                    self_.set("INDENT2", s(""));
                }
                if pl::num_eq(&options.get("Replace"), &n(2)) {
                    num_set = pl::add(&num_set, &n(1));
                    continue 'tl;
                }
            }

            if !val.is_undef() {
                // we are editing this tag, so create a NEW_VALUE hash entry
                let deleting = options.get("DelValue").truthy() && !shift.truthy();
                let nv_hash = gnvh(it, vec![self_.clone(), tag_info.clone(), write_group.clone(), s("create"), options.get("ProtectSaved"), Value::from(deleting)])?;
                // ignore new values protected with ProtectSaved
                if !nv_hash.truthy() {
                    num_set = pl::add(&num_set, &n(1)); // (increment $numSet to avoid warning)
                    continue 'tl;
                }
                if tag_info.get("List").truthy() && !options.get("Replace").truthy() {
                    nv_hash.set("NoReplace", n(1));
                }
                nv_hash.set("WantGroup", want_group.clone());
                if edit_only.truthy() {
                    nv_hash.set("EditOnly", n(1));
                }
                // save maker note fixup information if writing maker notes
                if tag_info.get("MakerNotes").truthy() {
                    nv_hash.set("MAKER_NOTE_FIXUP", options.get("Fixup"));
                }
                if create_only {
                    // create only (never edit)
                    // empty item in DelValue list to never edit existing value
                    nv_hash.set("DelValue", Value::new_array(vec![s("")]));
                    nv_hash.set("CreateOnly", n(1));
                } else if options.get("DelValue").truthy() || add_value.truthy() || shift.truthy() {
                    // flag any AddValue or DelValue by creating the DelValue list
                    if !nv_hash.get("DelValue").truthy() {
                        nv_hash.set("DelValue", Value::new_array(Vec::new()));
                    }
                    if shift.truthy() {
                        // add shift value to list
                        nv_hash.set("Shift", val.clone());
                    } else if options.get("DelValue").truthy() {
                        // don't create if we are replacing a specific value
                        if !(pl::str_eq(&val, "") || tag_info.get("List").truthy()) {
                            nv_hash.set("IsCreating", n(0));
                        }
                        // add delete value to list
                        let vals = if val.ref_type() == "ARRAY" { val.items() } else { vec![val.clone()] };
                        for v in &vals {
                            nv_hash.get("DelValue").push(v.clone());
                        }
                        if gt(&verbose, 1) {
                            let verb = if permanent.truthy() { "Replacing" } else { "Deleting" };
                            let from_list = if tag_info.get("List").truthy() { " from list" } else { "" };
                            for mut v in vals {
                                if v.ref_type() == "HASH" {
                                    crate::runtime::require(it, "Image/ExifTool/XMPStruct.pl")?;
                                    v = pl::call(it, "Image::ExifTool::XMP::SerializeStruct", vec![self_.clone(), v])?;
                                }
                                print_out(it, &out, cat(&[&self_.get("INDENT2"), &s(&format!("{verb} ")), &wt, &s(from_list), &s(" if value is '"), &v, &s("'\n")]))?;
                            }
                        }
                    }
                }
                // set priority flag to add only the high priority info
                // (will only create the priority tag if it doesn't exist,
                //  others get changed only if they already exist)
                let mut prf = if !tag_info.get("Preferred").is_undef() { tag_info.get("Preferred") } else { viv(tag_info, "Table").get("PREFERRED") };
                // hack to prefer only a single tag in the QuickTime group
                if pl::str_eq(&viv(&viv(tag_info, "Table"), "GROUPS").get("0"), "QuickTime") && pl::lt(&tag_priority.get(&tik), &highest_qt.get(lc_tag.to_bytes())) {
                    prf = n(0);
                }
                if pref.get(&tik).truthy() || prf.truthy() {
                    if permanent.truthy() || shift.truthy() {
                        // don't create permanent or Shift-ed tag but define IsCreating
                        // so we know that it is the preferred tag
                        nv_hash.set("IsCreating", n(0));
                    } else if (tag_info.get("List").truthy() && !options.get("DelValue").truthy())
                        || !(nv_hash.get("DelValue").truthy() && !nv_hash.get("DelValue").is_empty())
                        // also create tag if any DelValue value is empty ('')
                        || nv_hash.get("DelValue").items().iter().any(|v| re(r"^$", "").is_match(v))
                    {
                        nv_hash.set("IsCreating", if edit_only.truthy() { n(0) } else if edit_group.truthy() { n(2) } else { n(1) });
                        // add to hash of groups where this tag is being created
                        if !create_groups.truthy() {
                            create_groups = options.get("CreateGroups");
                            if !create_groups.truthy() {
                                create_groups = Value::new_hash();
                            }
                        }
                        let g0 = pl::method(it, &self_, "GetGroup", vec![tag_info.clone(), n(0)])?;
                        create_groups.set(g0.to_bytes(), n(1));
                        nv_hash.set("CreateGroups", create_groups.clone());
                    }
                }
                if nv_hash.get("IsCreating").truthy() {
                    let dg = self_.get("DEL_GROUP");
                    if !hash_rv(&dg)?.is_empty() {
                        let mut grps: Vec<Value> = Vec::new();
                        for grp in keys(&dg) {
                            if pl::num_eq(&dg.get(grp.to_bytes()), &n(2)) {
                                continue;
                            }
                            // set flag indicating tags were written after this group was deleted
                            dg.set(grp.to_bytes(), n(2));
                            grps.push(grp);
                        }
                        if gt(&verbose, 1) && !grps.is_empty() {
                            let grps = sort_str(grps);
                            print_out(it, &out, cat(&[&s("  Writing new tags after deleting groups: "), &pl::join(" ", &grps), &s("\n")]))?;
                        }
                    }
                } else if create_only {
                    no_create = if permanent.truthy() {
                        s("permanent")
                    } else if tag_info.get("Avoid").truthy() {
                        s("avoided")
                    } else {
                        s("")
                    };
                    if !no_create.truthy() {
                        no_create = if shift.truthy() { s("shifting") } else { s("not preferred") };
                    }
                    if gt(&verbose, 2) {
                        print_out(it, &out, cat(&[&s("Not creating "), &wt, &s(" ("), &no_create, &s(")\n")]))?;
                    }
                    continue 'tl; // nothing to do (not creating and not editing)
                }
                if shift.truthy() || !options.get("DelValue").truthy() {
                    if !nv_hash.get("Value").truthy() {
                        nv_hash.set("Value", Value::new_array(Vec::new()));
                    }
                    let nvv = nv_hash.get("Value");
                    let vals = if val.ref_type() == "ARRAY" { val.items() } else { vec![val.clone()] };
                    if !tag_info.get("List").truthy() {
                        // not a List tag -- overwrite existing value
                        nvv.set_at(0, val.clone());
                    } else if !nv_hash.get("AddBefore").is_undef() && pl::ge(&n(nvv.len() as i64), &nv_hash.get("AddBefore")) {
                        // values from a later argument have been added (ie. Replace=0)
                        // to this list, so the new values should come before these
                        if let Value::ArrayRef(arr) = &nvv {
                            let mut arr = arr.borrow_mut();
                            let pos = (arr.len() as i64 - nv_hash.get("AddBefore").to_i64()).max(0) as usize;
                            for (j, v) in vals.into_iter().enumerate() {
                                arr.insert(pos + j, v);
                            }
                        }
                    } else {
                        // add at end of existing list
                        for v in vals {
                            nvv.push(v);
                        }
                    }
                    if gt(&verbose, 1) {
                        let if_exists;
                        if tag_info.get("IsComposite").truthy() {
                            // (composite tags don't technically exist in the file)
                            if tag_info.get("WriteAlso").truthy() {
                                if_exists = cat(&[&s(" (+"), &pl::join(",+", &sort_str(keys(&tag_info.get("WriteAlso")))), &s("):")]);
                            } else {
                                if_exists = s("");
                            }
                        } else {
                            let is_creating = nv_hash.get("IsCreating");
                            if_exists = if is_creating.truthy() {
                                if create_only {
                                    if pl::num_eq(&is_creating, &n(2)) {
                                        cat(&[&s(" if "), &write_group, &s(" exists and tag doesn't")])
                                    } else {
                                        s(" if tag doesn't exist")
                                    }
                                } else if pl::num_eq(&is_creating, &n(2)) {
                                    cat(&[&s(" if "), &write_group, &s(" exists")])
                                } else {
                                    s("")
                                }
                            } else if nv_hash.get("DelValue").truthy() && !nv_hash.get("DelValue").is_empty() {
                                s(" if tag was deleted")
                            } else {
                                s(" if tag exists")
                            };
                        }
                        let verb = if shift.truthy() {
                            "Shifting"
                        } else if add_value.truthy() {
                            "Adding"
                        } else {
                            "Writing"
                        };
                        print_out(it, &out, cat(&[&self_.get("INDENT2"), &s(&format!("{verb} ")), &wt, &if_exists, &s("\n")]))?;
                    }
                }
            } else if permanent.truthy() {
                err = cat(&[&s("Can't delete Permanent tag "), &wt]);
                if gt(&verbose, 1) {
                    print_out(it, &out, pl::concat(&err, &s("\n")))?;
                }
                continue 'tl;
            } else if add_value.truthy() || options.get("DelValue").truthy() {
                if gt(&verbose, 1) {
                    print_out(it, &out, s("Adding/Deleting nothing does nothing\n"))?;
                }
                continue 'tl;
            } else {
                // create empty new value hash entry to delete this tag
                gnvh(it, vec![self_.clone(), tag_info.clone(), write_group.clone(), s("delete")])?;
                let nv_hash = gnvh(it, vec![self_.clone(), tag_info.clone(), write_group.clone(), s("create")])?;
                nv_hash.set("WantGroup", want_group.clone());
                if gt(&verbose, 1) {
                    print_out(it, &out, cat(&[&self_.get("INDENT2"), &s("Deleting "), &wt, &s("\n")]))?;
                }
            }
            if set_tags.truthy() {
                set_tags.set(&tik, n(1));
            }
            if pref.get(&tik).truthy() {
                priority_set = true;
            }
        }
        // WriteAlso:
        num_set = pl::add(&num_set, &n(1));
        // also write related tags
        let write_also = tag_info.get("WriteAlso");
        if write_also.truthy() {
            self_.set("INDENT2", s("+")); // indicate related tag with a leading "+"
            let wgrp = if tag_info.get("WriteGroup").truthy() && pl::str_eq(&tag_info.get("WriteGroup"), "All") && write_group.truthy() { pl::concat(&write_group, &s(":")) } else { s("") };
            let h = set_warning_ref(it);
            let r: ER<()> = pl::local_sig_warn(it, h, |it| {
                for wtag in sort_str(keys(&write_also)) {
                    let opts = Value::new_hash();
                    opts.set("Type", s("ValueConv"));
                    opts.set("Protected", ops::bit_or(&protected, &n(0x02)));
                    opts.set("AddValue", add_value.clone());
                    opts.set("DelValue", options.get("DelValue"));
                    opts.set("Shift", options.get("Shift"));
                    opts.set("Replace", options.get("Replace")); // handle lists properly
                    opts.set("CreateGroups", create_groups.clone());
                    opts.set("SetTags", also_wrote.clone()); // remember tags already written
                    it.set_scalar(EVAL_WARNING, Value::Undef);
                    //### eval WriteAlso ($val, %opts)
                    let opts_rc: Rc<RefCell<Option<crate::interp::eval::HashRc>>> = Rc::new(RefCell::new(None));
                    let oc = opts_rc.clone();
                    let oh = match &opts {
                        Value::HashRef(r) => r.borrow().clone(),
                        _ => Default::default(),
                    };
                    let (mut v_val, mut sv, mut ti, mut tg, mut wg, mut wt_) = (val.clone(), self_.clone(), tag_info.clone(), tag.clone(), write_group.clone(), wtag.clone());
                    let mut v = eval_vars(
                        it,
                        &write_also.get(wtag.to_bytes()),
                        &mut [("val", &mut v_val), ("self", &mut sv), ("tagInfo", &mut ti), ("tag", &mut tg), ("writeGroup", &mut wg), ("wtag", &mut wt_)],
                        move |sc| {
                            *oc.borrow_mut() = Some(sc.hash("opts", oh));
                        },
                        Want::Scalar,
                    )?;
                    let opts = match opts_rc.borrow().clone() {
                        Some(h) => Value::HashRef(h),
                        None => opts,
                    };
                    // we wanted to do the eval in case there are side effect, but we
                    // don't want to write a value for a tag that is being deleted:
                    if val.is_undef() {
                        v = Value::Undef;
                    }
                    let e = eval_error(it);
                    if e.truthy() {
                        it.set_scalar(EVAL_WARNING, e);
                    }
                    if !it.get_scalar(EVAL_WARNING).truthy() {
                        let (n_, ew) = snv(it, &self_, pl::concat(&wgrp, &wtag), v, &opts)?;
                        it.set_scalar(EVAL_WARNING, ew);
                        num_set = pl::add(&num_set, &n_);
                        // count this as being set if any related tag is set
                        if n_.truthy() && pref.get(&tik).truthy() {
                            priority_set = true;
                        }
                    }
                    if it.get_scalar(EVAL_WARNING).truthy() && (!err.truthy() || gt(&verbose, 2)) {
                        let mut str = clean_warning(it)?;
                        if str.truthy() {
                            if !re(r" for [-\w:]+$", "").is_match(&str) {
                                str = cat(&[&str, &s(" for "), &wtag]);
                            }
                            str = cat(&[&str, &s(" in "), &wt, &s(" (WriteAlso)")]);
                            if !err.truthy() {
                                err = str.clone();
                            }
                            if gt(&verbose, 2) {
                                print_out(it, &out, pl::concat(&str, &s("\n")))?;
                            }
                        }
                    }
                }
                Ok(())
            });
            r?;
            self_.set("INDENT2", s(""));
        }
    }
    // print warning if we couldn't set our priority tag
    if !err.is_undef() && !priority_set {
        if err.truthy() && want != Want::List {
            warn_nl(it, &err)?;
        }
    } else if !num_set.truthy() {
        let pre = if want_group.truthy() { pl::concat(&want_group, &s(":")) } else { s("") };
        if was_protected.truthy() {
            verbose = n(0); // we already printed this verbose message
            if !(options.get("Replace").truthy() && pl::num_eq(&options.get("Replace"), &n(2))) {
                err = cat(&[&s("Sorry, "), &pre, &tag, &s(" is "), &was_protected, &s(" for writing")]);
            }
        } else if !list_only.truthy() {
            if re(r"[?*]", "").is_match(&orig_tag) {
                if no_create.truthy() {
                    err = cat(&[&s("No tags matching 'pre"), &orig_tag, &s("' will be created")]);
                    verbose = n(0); // (already printed)
                } else if found_match > 0 {
                    err = cat(&[&s("Sorry, no writable tags matching '"), &pre, &orig_tag, &s("'")]);
                } else {
                    err = cat(&[&s("No matching tags for '"), &pre, &orig_tag, &s("'")]);
                }
            } else if no_create.truthy() {
                err = cat(&[&s("Not creating "), &pre, &tag]);
                verbose = n(0); // (already printed)
            } else if found_match > 0 {
                err = cat(&[&s("Sorry, "), &pre, &tag, &s(" is not writable")]);
            } else if want_group.truthy() && !matching_tags.is_empty() {
                err = cat(&[&s("Sorry, "), &pre, &tag, &s(" doesn't exist or isn't writable")]);
            } else {
                err = cat(&[&s("Tag '"), &pre, &tag, &s("' is not defined")]);
            }
        }
        if err.truthy() {
            if gt(&verbose, 2) {
                print_out(it, &out, pl::concat(&err, &s("\n")))?;
            }
            if want != Want::List {
                warn_nl(it, &err)?;
            }
        }
    } else if self_.get("CHECK_WARN").truthy() {
        err = self_.get("CHECK_WARN");
        if gt(&verbose, 2) {
            print_out(it, &out, pl::concat(&err, &s("\n")))?;
        }
    } else if err.truthy() && !verbose.truthy() {
        err = Value::Undef;
    }
    if want == Want::List {
        return Ok(vec![num_set, err]);
    }
    ret(num_set)
}

/// `UNIVERSAL::isa($v, CLASS)`
fn isa_(it: &mut Interp, v: &Value, class: &str) -> ER<bool> {
    Ok(pl::call(it, "UNIVERSAL::isa", vec![v.clone(), s(class)])?.truthy())
}

/// Write information back to file
/// Inputs: 0) ExifTool object reference,
///         1) input filename, file ref, RAF ref, or scalar ref (or '' or undef to create from scratch)
///         2) output filename, file ref, or scalar ref (or undef to overwrite)
///         3) optional output file type (required only if input file is not specified
///            and output file is a reference)
/// Returns: 1=file written OK, 2=file written but no changes made, 0=file write error
pub fn write_info(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    // local ($_, *EXIFTOOL_FILE2, *EXIFTOOL_OUTFILE);
    // (the interpreter's `local *FH` saves nothing: the handles are reopened)
    pl::local_topic(it, |it| write_info_body(it, a))
}

fn write_info_body(it: &mut Interp, a: Vec<Value>) -> ER<Vec<Value>> {
    let (self_, mut infile, mut outfile, mut out_type) = (arg(&a, 0), arg(&a, 1), arg(&a, 2), arg(&a, 3));
    let mut file_type_list: std::collections::VecDeque<Value> = Default::default();
    let (mut file_type, mut tiff_type, mut hdr, mut seek_err, mut type_, mut tmpfile) = (Value::Undef, Value::Undef, Value::Undef, false, Value::Undef, Value::Undef);
    let mut in_ref: Value;
    let (mut out_ref, mut close_in, mut close_out, mut out_pos, mut out_buff, mut erase_in, mut raf, mut file_ext) = (Value::Undef, false, false, Value::Undef, Value::Undef, false, Value::Undef, Value::Undef);
    let (mut hard_link, mut sym_link, mut test_name) = (Value::Undef, Value::Undef, Value::Undef);
    let old_raf = self_.get("RAF");
    let mut rtn_val = n(0);
    let error_v = |self_: &Value| viv(self_, "VALUE").get("Error");

    // initialize member variables
    method_void(it, &self_, "Init", vec![])?;
    self_.set("IsWriting", n(1));

    // first, save original file modify date if necessary
    // (do this now in case we are modifying file in place and shifting date)
    let (mut nv_hash, mut nv_hash2, mut original_time, mut create_time) = (Value::Undef, Value::Undef, Value::Undef, Value::Undef);
    let set_mod_date = !get_new_value_nvh(it, &self_, s("FileModifyDate"), &mut nv_hash)?.is_undef();
    let set_create_date = !get_new_value_nvh(it, &self_, s("FileCreateDate"), &mut nv_hash2)?.is_undef();
    let (mut _a_time, mut _m_time, mut c_time) = (Value::Undef, Value::Undef, Value::Undef);
    if set_mod_date && pl::lt(&overwriting(it, &self_, &nv_hash, None)?, &n(0)) && !infile.is_undef() && infile.ref_type() != "SCALAR" {
        let t = pl::method_list(it, &self_, "GetFileTime", vec![infile.clone()])?;
        (_a_time, _m_time, c_time) = (arg(&t, 0), arg(&t, 1), arg(&t, 2));
        original_time = _m_time.clone();
    }
    if set_create_date && pl::lt(&overwriting(it, &self_, &nv_hash2, None)?, &n(0)) && !infile.is_undef() && infile.ref_type() != "SCALAR" {
        if c_time.is_undef() {
            let t = pl::method_list(it, &self_, "GetFileTime", vec![infile.clone()])?;
            (_a_time, _m_time, c_time) = (arg(&t, 0), arg(&t, 1), arg(&t, 2));
        }
        create_time = c_time.clone();
    }
    //
    // do quick in-place change of file dir/name or date if that is all we are doing
    //
    let cnv = count_new_values(it, vec![self_.clone()], Want::List)?;
    let (num_new, num_pseudo) = (arg(&cnv, 0), arg(&cnv, 1));
    if outfile.is_undef() && !infile.is_undef() {
        hard_link = get_new_value_s(it, &self_, s("HardLink"))?;
        sym_link = get_new_value_s(it, &self_, s("SymLink"))?;
        test_name = get_new_value_s(it, &self_, s("TestName"))?;
        if !hard_link.is_undef() && length_of(&hard_link) == 0 {
            hard_link = Value::Undef;
        }
        if !sym_link.is_undef() && length_of(&sym_link) == 0 {
            sym_link = Value::Undef;
        }
        if !test_name.is_undef() && length_of(&test_name) == 0 {
            test_name = Value::Undef;
        }
        let new_file_name_v = get_new_value_nvh(it, &self_, s("FileName"), &mut nv_hash)?;
        let mut new_dir = get_new_value_s(it, &self_, s("Directory"))?;
        if !new_dir.is_undef() && length_of(&new_dir) > 0 {
            if !re(r"/$", "").is_match(&new_dir) {
                new_dir = pl::concat(&new_dir, &s("/"));
            }
        } else {
            new_dir = Value::Undef;
        }
        if pl::num_eq(&num_new, &num_pseudo) {
            rtn_val = n(2);
            if (!new_file_name_v.is_undef() || !new_dir.is_undef()) && !infile.is_ref() {
                let result = file_name(it, vec![self_.clone(), infile.clone()])?;
                if pl::gt(&result, &n(0)) {
                    infile = self_.get("NewName"); // file name changed
                    rtn_val = n(1);
                } else if pl::lt(&result, &n(0)) {
                    return ret(n(0)); // don't try to do anything else
                }
            }
            if !infile.is_ref() || isa_(it, &infile, "GLOB")? {
                if set_mod_date && pl::gt(&file_modify_date(it, vec![self_.clone(), infile.clone()])?, &n(0)) {
                    rtn_val = n(1);
                }
                if set_create_date && pl::gt(&file_modify_date(it, vec![self_.clone(), infile.clone(), Value::Undef, s("FileCreateDate")])?, &n(0)) {
                    rtn_val = n(1);
                }
                if pl::gt(&system_tags(it, &self_, &infile)?, &n(0)) {
                    rtn_val = n(1);
                }
            }
            if !hard_link.is_undef() || !sym_link.is_undef() || !test_name.is_undef() {
                if hard_link.truthy() && file_name(it, vec![self_.clone(), infile.clone(), hard_link.clone(), s("HardLink")])?.truthy() {
                    rtn_val = n(1);
                }
                if sym_link.truthy() && file_name(it, vec![self_.clone(), infile.clone(), sym_link.clone(), s("SymLink")])?.truthy() {
                    rtn_val = n(1);
                }
                if test_name.truthy() && file_name(it, vec![self_.clone(), infile.clone(), test_name.clone(), s("Test")])?.truthy() {
                    rtn_val = n(1);
                }
            }
            return ret(rtn_val);
        } else if !new_file_name_v.is_undef() && length_of(&new_file_name_v) > 0 {
            // can't simply rename file, so just set the output name if new FileName
            // --> in this case, must erase original copy
            if infile.is_ref() {
                outfile = new_file_name_v.clone();
                // can't delete original
            } else if overwriting(it, &self_, &nv_hash, Some(infile.clone()))?.truthy() {
                outfile = new_file_name(it, &infile, &new_file_name_v)?;
                erase_in = true; // delete original
            }
        }
        // set new directory if specified
        if !new_dir.is_undef() {
            if outfile.is_undef() && !infile.is_ref() {
                outfile = infile.clone();
            }
            if !outfile.is_undef() {
                outfile = new_file_name(it, &outfile, &new_dir)?;
                if !infile.is_ref() {
                    erase_in = true;
                }
            }
        }
    }
    //
    // set up input file
    //
    if infile.is_ref() {
        in_ref = infile.clone();
        if isa_(it, &in_ref, "GLOB")? {
            snippet1(it, "seek($r, 0, 0)", &[("r", in_ref.clone())])?; // make sure we are at the start of the file
        } else if isa_(it, &in_ref, "File::RandomAccess")? {
            method_void(it, &in_ref, "Seek", vec![n(0)])?;
            raf = in_ref.clone();
        } else {
            let mut doit = opt(&self_, "EncodeHangs").truthy();
            if !doit {
                let v = in_ref.deref();
                let r = pl::eval_block(it, |it| {
                    crate::runtime::require(it, "Encode")?;
                    pl::call(it, "Encode::is_utf8", vec![v])
                })?;
                doit = r.map(|x| x.truthy()).unwrap_or(false) || eval_error(it).truthy();
            }
            if doit {
                let h = set_warning_ref(it);
                let r0 = in_ref.clone();
                let buff = pl::local_sig_warn(it, h, |it| {
                    // convert image data from UTF-8 to character stream if necessary
                    if opt(&self_, "EncodeHangs").truthy() || eval_error(it).truthy() {
                        pl::pack("C*", &pl::unpack("C0C*", &r0.deref())?)
                    } else {
                        pl::call(it, "Encode::encode", vec![s("utf8"), r0.deref()])
                    }
                })?;
                if !outfile.is_undef() {
                    in_ref = pl::sref(buff);
                } else {
                    pl::set_deref(&in_ref, buff)?;
                }
            }
        }
    } else if !infile.is_undef() && !pl::str_eq(&infile, "") {
        // write to a temporary file if no output file given
        if outfile.is_undef() {
            tmpfile = pl::concat(&infile, &s("_exiftool_tmp"));
            outfile = tmpfile.clone();
        }
        let fh = glob_ref(it, "EXIFTOOL_FILE2")?;
        if pl::method(it, &self_, "Open", vec![fh.clone(), infile.clone()])?.truthy() {
            file_ext = pl::call(it, "Image::ExifTool::GetFileExtension", vec![infile.clone()])?;
            file_type = pl::call(it, "Image::ExifTool::GetFileType", vec![infile.clone()])?;
            file_type_list = pl::call_list(it, "Image::ExifTool::GetFileType", vec![infile.clone()])?.into();
            tiff_type = pl::call(it, "Image::ExifTool::GetFileExtension", vec![infile.clone()])?;
            self_.set("FILE_EXT", tiff_type.clone());
            method_void(it, &self_, "VPrint", vec![n(0), cat(&[&s("Rewriting "), &infile, &s("...\n")])])?;
            in_ref = fh;
            close_in = true; // we must close the file since we opened it
        } else {
            error_(it, &self_, s("Error opening file"))?;
            return ret(n(0));
        }
    } else if outfile.is_undef() {
        error_(it, &self_, s("WriteInfo(): Must specify infile or outfile\n"))?;
        return ret(n(0));
    } else {
        // create file from scratch
        if !(out_type.truthy() || outfile.is_ref()) {
            out_type = pl::call(it, "Image::ExifTool::GetFileExtension", vec![outfile.clone()])?;
        }
        if pl::call(it, "Image::ExifTool::CanCreate", vec![out_type.clone()])?.truthy() {
            if re(r"g", "i").is_match(&opt(&self_, "WriteMode")) {
                file_type = out_type.clone(); // use output file type if no input file
                tiff_type = out_type.clone();
                infile = pl::concat(&file_type, &s(" file")); // make bogus file name
                method_void(it, &self_, "VPrint", vec![n(0), cat(&[&s("Creating "), &infile, &s("...\n")])])?;
                in_ref = pl::sref(s("")); // set $inRef to reference to empty data
            } else {
                error_(it, &self_, cat(&[&s("Not creating new "), &out_type, &s(" file (disallowed by WriteMode)")]))?;
                return ret(n(0));
            }
        } else if out_type.truthy() {
            error_(it, &self_, cat(&[&s("Can't create "), &out_type, &s(" files")]))?;
            return ret(n(0));
        } else {
            error_(it, &self_, s("Can't create file (unknown type)"))?;
            return ret(n(0));
        }
    }
    if file_type_list.is_empty() {
        if file_type.truthy() {
            file_type_list = vec![file_type.clone()].into();
        } else {
            file_type_list = pl::pkg_array(it, "Image::ExifTool::fileTypes").items().into();
            tiff_type = s("TIFF");
        }
    }
    //
    // set up output file
    //
    if outfile.is_ref() {
        out_ref = outfile.clone();
        if isa_(it, &out_ref, "GLOB")? {
            snippet1(it, "binmode($r)", &[("r", out_ref.clone())])?;
            out_pos = snippet1(it, "tell($r)", &[("r", out_ref.clone())])?;
        } else {
            // initialize our output buffer if necessary
            if out_ref.deref().is_undef() {
                pl::set_deref(&out_ref, s(""))?;
            }
            out_pos = n(length_of(&out_ref.deref()));
        }
    } else if outfile.is_undef() {
        // editing in place, so write to memory first
        // (only when infile is a file ref or scalar ref)
        if raf.truthy() {
            error_(it, &self_, s("Can't edit File::RandomAccess object in place"))?;
            return ret(n(0));
        }
        out_buff = pl::sref(s("")); // (\$outBuff)
        out_ref = out_buff.clone();
        out_pos = n(0);
    } else if pl::method(it, &self_, "Exists", vec![outfile.clone(), n(1)])?.truthy() {
        error_(it, &self_, pl::concat(&s("File already exists: "), &outfile))?;
    } else {
        let fh = glob_ref(it, "EXIFTOOL_OUTFILE")?;
        if pl::method(it, &self_, "Open", vec![fh.clone(), outfile.clone(), s(">")])?.truthy() {
            out_ref = fh;
            close_out = true; // we must close $outRef
            snippet1(it, "binmode($r)", &[("r", out_ref.clone())])?;
            out_pos = n(0);
        } else {
            let tmp = if tmpfile.truthy() { " temporary" } else { "" };
            error_(it, &self_, cat(&[&s(&format!("Error creating{tmp} file: ")), &outfile]))?;
        }
    }
    let outbuff_defined = |ob: &Value| !ob.is_undef() && !ob.deref().is_undef();
    //
    // write the file
    //
    if !error_v(&self_).truthy() {
        'w: {
            // create random access file object (disable seek test in case of straight copy)
            if !raf.truthy() {
                raf = pl::method(it, &s("File::RandomAccess"), "new", vec![in_ref.clone(), n(1)])?;
            }
            method_void(it, &raf, "BinMode", vec![])?;
            if pl::num_eq(&num_new, &num_pseudo) {
                rtn_val = n(1);
                // just do a straight copy of the file (no "real" tags are being changed)
                loop {
                    let mut rd = [Value::Undef, n(65536)];
                    if !pl::method_mut(it, &raf, "Read", &mut rd, Want::Scalar)?.pop().unwrap_or_default().truthy() {
                        break;
                    }
                    if !pl::call(it, "Image::ExifTool::Write", vec![out_ref.clone(), rd[0].clone()])?.truthy() {
                        rtn_val = n(-1);
                        break;
                    }
                }
                break 'w;
            } else if !infile.is_ref() && (pl::str_eq(&infile, "-") || re(r"\|$", "").is_match(&infile)) {
                // patch for Windows command shell pipe
                raf.set("TESTED", n(-1)); // force buffering
            } else {
                method_void(it, &raf, "SeekTest", vec![])?;
            }
            // $raf->Debug() and warn "  RAF debugging enabled!\n";
            let in_pos = pl::method(it, &raf, "Tell", vec![])?;
            self_.set("RAF", raf.clone());
            let dir_info = Value::new_hash();
            dir_info.set("RAF", raf.clone());
            dir_info.set("OutFile", out_ref.clone());
            let mut rd = [Value::Undef, n(1024)];
            let ok = pl::method_mut(it, &raf, "Read", &mut rd, Want::Scalar)?.pop().unwrap_or_default();
            hdr = rd[0].clone();
            if !ok.truthy() {
                hdr = s("");
            }
            if !pl::method(it, &raf, "Seek", vec![in_pos.clone(), n(0)])?.truthy() {
                seek_err = true;
            }
            let mut wrong_type = false;
            let magic_number = pl::pkg_hash(it, "Image::ExifTool::magicNumber");
            let writable_type = my_hash(it, "writableType");
            while !seek_err {
                type_ = file_type_list.pop_front().unwrap_or_default();
                // do quick test to see if this is the right file type
                let mn = magic_number.get(type_.to_bytes());
                if mn.truthy() && length_of(&hdr) > 0 {
                    let mut p = b"^".to_vec();
                    p.extend_from_slice(&mn.to_bytes());
                    if !pl::re_dyn(&p, "s")?.is_match(&hdr) {
                        if !file_type_list.is_empty() {
                            continue;
                        }
                        wrong_type = true;
                        break;
                    }
                }
                // save file type in member variable
                self_.set("FILE_TYPE", type_.clone());
                viv_array(&self_, "PATH").set_at(0, type_.clone());
                dir_info.set("Parent", type_.clone());
                // determine which directories we must write for this file type
                init_write_dirs(it, vec![self_.clone(), type_.clone()], Want::Void)?;
                if pl::str_eq(&type_, "JPEG") || pl::str_eq(&type_, "EXV") {
                    rtn_val = pl::method(it, &self_, "WriteJPEG", vec![dir_info.clone()])?;
                } else if pl::str_eq(&type_, "TIFF") {
                    // disallow writing of some TIFF-based RAW images:
                    let nwf = viv(&pl::pkg_hash(it, "Image::ExifTool::noWriteFile"), "TIFF").items();
                    if !grep_dyn(&anchored(&tiff_type), "", &nwf)?.is_empty() {
                        file_type = tiff_type.clone();
                        rtn_val = Value::Undef;
                    } else {
                        if pl::str_eq(&tiff_type, "FFF") {
                            // (see https://exiftool.org/forum/index.php?topic=10848.0)
                            method_void(it, &self_, "Error", vec![s("Phocus may not properly update previews of edited FFF images"), n(1)])?;
                        }
                        dir_info.set("Parent", tiff_type.clone());
                        rtn_val = pl::method(it, &self_, "ProcessTIFF", vec![dir_info.clone()])?;
                    }
                } else if writable_type.exists(type_.to_bytes()) {
                    let wt = writable_type.get(type_.to_bytes());
                    let (module, func);
                    if wt.ref_type() == "ARRAY" {
                        let m0 = wt.at(0);
                        module = if m0.truthy() { m0 } else { type_.clone() };
                        func = wt.at(1);
                    } else {
                        module = if wt.truthy() { wt } else { type_.clone() };
                        func = Value::Undef;
                    }
                    crate::runtime::require(it, &format!("Image/ExifTool/{}.pm", module.to_text()))?;
                    let f = if func.truthy() { func } else { pl::concat(&s("Process"), &type_) };
                    let func = cat(&[&s("Image::ExifTool::"), &module, &s("::"), &f]);
                    rtn_val = pl::call(it, &func.to_text(), vec![self_.clone(), dir_info.clone()])?;
                } else if pl::str_eq(&type_, "ORF") || pl::str_eq(&type_, "RAW") {
                    rtn_val = pl::method(it, &self_, "ProcessTIFF", vec![dir_info.clone()])?;
                } else if pl::str_eq(&type_, "EXIF") {
                    // go through WriteDirectory so block writes, etc are handled
                    let tag_table_ptr = pl::call(it, "Image::ExifTool::GetTagTable", vec![s("Image::ExifTool::Exif::Main")])?;
                    let wt = Value::Code(it.sub_ref("Image::ExifTool::WriteTIFF"));
                    let buff = write_directory(it, vec![self_.clone(), dir_info.clone(), tag_table_ptr, wt], Want::Scalar)?.pop().unwrap_or_default();
                    if !buff.is_undef() {
                        rtn_val = if pl::call(it, "Image::ExifTool::Write", vec![out_ref.clone(), buff])?.truthy() { n(1) } else { n(-1) };
                    } else {
                        rtn_val = n(0);
                    }
                } else {
                    rtn_val = Value::Undef; // flag that we don't write this type of file
                }
                // all done unless we got the wrong type
                if rtn_val.truthy() {
                    break;
                }
                if file_type_list.is_empty() {
                    break;
                }
                // seek back to original position in files for next try
                if !pl::method(it, &raf, "Seek", vec![in_pos.clone(), n(0)])?.truthy() {
                    seek_err = true;
                    break;
                }
                if isa_(it, &out_ref, "GLOB")? {
                    snippet1(it, "seek($r, 0, $p)", &[("r", out_ref.clone()), ("p", out_pos.clone())])?;
                } else {
                    let v = pl::substr(&out_ref.deref(), 0, Some(out_pos.to_i64()));
                    pl::set_deref(&out_ref, v)?;
                }
            }
            // print file format errors
            if !rtn_val.truthy() {
                let mut err = Value::Undef;
                if seek_err {
                    err = s("Error seeking in file");
                } else if file_type.truthy() && !rtn_val.is_undef() {
                    if error_v(&self_).truthy() {
                        // existing error message will do
                    } else if pl::str_eq(&file_type, "RAW") {
                        err = s("Writing this type of RAW file is not supported");
                    } else if wrong_type {
                        let type2 = if file_ext.truthy() {
                            file_ext.clone()
                        } else if pl::str_eq(&file_type, "TIFF") {
                            tiff_type.clone()
                        } else {
                            file_type.clone()
                        };
                        err = pl::concat(&s("Not a valid "), &type2);
                        // do a quick check to see what this file looks like
                        for t in pl::pkg_array(it, "Image::ExifTool::fileTypes").items() {
                            let mn = magic_number.get(t.to_bytes());
                            if !mn.truthy() {
                                continue;
                            }
                            let mut p = b"^".to_vec();
                            p.extend_from_slice(&mn.to_bytes());
                            if !pl::re_dyn(&p, "s")?.is_match(&hdr) {
                                continue;
                            }
                            err = cat(&[&err, &s(" (looks more like a "), &t, &s(")")]);
                            break;
                        }
                    } else {
                        err = s("Format error in file");
                    }
                } else if file_type.truthy() {
                    // get specific type of file from extension
                    if infile.truthy() && pl::call(it, "Image::ExifTool::GetFileType", vec![infile.clone()])?.truthy() {
                        file_type = pl::call(it, "Image::ExifTool::GetFileExtension", vec![infile.clone()])?;
                    }
                    err = cat(&[&s("Writing of "), &file_type, &s(" files is not yet supported")]);
                } else {
                    err = s("Writing of this type of file is not supported");
                }
                if err.truthy() {
                    error_(it, &self_, err)?;
                }
                rtn_val = n(0); // (in case it was undef)
            }
            // $raf->Close();  # only used to force debug output
        }
    }
    // don't return success code if any error occurred
    if pl::gt(&rtn_val, &n(0)) {
        if out_type.truthy() && type_.truthy() && !eq(&out_type, &type_) {
            let types = pl::call_list(it, "Image::ExifTool::GetFileType", vec![out_type.clone()])?;
            if grep_dyn(&anchored(&type_), "", &types)?.is_empty() {
                error_(it, &self_, cat(&[&s("Can't create "), &out_type, &s(" file from "), &type_]))?;
                rtn_val = n(0);
            }
        }
        if pl::gt(&rtn_val, &n(0)) && !pl::call(it, "Image::ExifTool::Tell", vec![out_ref.clone()])?.truthy() && !error_v(&self_).truthy() {
            // don't write a file with zero length
            if !hdr.is_undef() && length_of(&hdr) > 0 {
                if type_.is_undef() {
                    type_ = s("<unk>");
                }
                error_(it, &self_, cat(&[&s("Can't delete all meta information from "), &type_, &s(" file")]))?;
            } else {
                error_(it, &self_, s("Nothing to write"))?;
            }
        }
        if error_v(&self_).truthy() {
            rtn_val = n(0);
        }
    }

    // rewrite original file in place if required
    if outbuff_defined(&out_buff) {
        if !pl::gt(&rtn_val, &n(0)) || !self_.get("CHANGED").truthy() {
            // nothing changed, so no need to write $outBuff
        } else if isa_(it, &in_ref, "GLOB")? {
            let ok = snippet1(
                it,
                "my $len = length($b); my $size; (seek($r, 0, 2) and ($size = tell $r) >= 0 and seek($r, 0, 0) and print $r $b and ($len >= $size or eval { truncate($r, $len) })) ? 1 : 0",
                &[("r", in_ref.clone()), ("b", out_buff.deref())],
            )?;
            if !ok.truthy() {
                rtn_val = n(-1);
            }
        } else {
            pl::set_deref(&in_ref, out_buff.deref())?; // replace original data
        }
        pl::set_deref(&out_buff, s(""))?; // free memory but leave $outBuff defined
    }
    // close input file if we opened it
    if close_in {
        // errors on input file are significant if we edited the file in place
        let closed = snippet1(it, "close($r)", &[("r", in_ref.clone())])?.truthy();
        if !(closed || !outbuff_defined(&out_buff)) && rtn_val.truthy() {
            rtn_val = n(-1);
        }
        if pl::gt(&rtn_val, &n(0)) {
            // copy Mac OS resource fork if it exists
            let rsrc = pl::concat(&infile, &s("/..namedfork/rsrc"));
            if pl::str_eq(&os(it), "darwin") && snippet1(it, "-s $f", &[("f", rsrc.clone())])?.truthy() {
                if viv(&self_, "DEL_GROUP").get("RSRC").truthy() {
                    method_void(it, &self_, "VPrint", vec![n(0), s("Deleting Mac OS resource fork\n")])?;
                    pre_inc(&self_, "CHANGED");
                } else {
                    method_void(it, &self_, "VPrint", vec![n(0), s("Copying Mac OS resource fork\n")])?;
                    let mut err = Value::Undef;
                    // local (*SRC, *DST);
                    let src = glob_ref(it, "SRC")?;
                    if pl::method(it, &self_, "Open", vec![src, rsrc.clone()])?.truthy() {
                        let dst = glob_ref(it, "DST")?;
                        if pl::method(it, &self_, "Open", vec![dst, pl::concat(&outfile, &s("/..namedfork/rsrc")), s(">")])?.truthy() {
                            err = snippet1(
                                it,
                                "my ($buf, $err); binmode SRC; binmode DST; while (read SRC, $buf, 65536) { print DST $buf or $err = 'copying', last; } close DST or $err or $err = 'closing'; $err",
                                &[],
                            )?;
                        } else {
                            // (this is normal if the destination filesystem isn't Mac OS)
                            warn_(it, &self_, s("Error creating Mac OS resource fork"))?;
                        }
                        perl_snippet(it, "close SRC", &[], Want::Void)?;
                    } else {
                        err = s("opening");
                    }
                    if err.truthy() && pl::method(it, &self_, "Error", vec![cat(&[&s("Error "), &err, &s(" Mac OS resource fork")]), n(2)])?.truthy() {
                        rtn_val = n(0);
                    }
                }
            }
            // erase input file if renaming while editing information in place
            if erase_in && !pl::method(it, &self_, "Unlink", vec![infile.clone()])?.truthy() {
                warn_(it, &self_, s("Error erasing original file"))?;
            }
        }
    }
    // close output file if we created it
    if close_out {
        // close file and set $rtnVal to -1 if there was an error
        if !snippet1(it, "close($r)", &[("r", out_ref.clone())])?.truthy() && rtn_val.truthy() {
            rtn_val = n(-1);
        }
        // erase the output file if we weren't successful
        if !pl::gt(&rtn_val, &n(0)) {
            method_void(it, &self_, "Unlink", vec![outfile.clone()])?;
        // else rename temporary file if necessary
        } else if tmpfile.truthy() {
            copy_file_attrs(it, vec![self_.clone(), infile.clone(), tmpfile.clone()], Want::Void)?; // copy attributes to new file
            if !pl::method(it, &self_, "Rename", vec![tmpfile.clone(), infile.clone()])?.truthy() {
                // some filesystems won't overwrite with 'rename', so try erasing original
                if !pl::method(it, &self_, "Unlink", vec![infile.clone()])?.truthy() {
                    method_void(it, &self_, "Unlink", vec![tmpfile.clone()])?;
                    error_(it, &self_, s("Error renaming temporary file"))?;
                    rtn_val = n(0);
                } else if !pl::method(it, &self_, "Rename", vec![tmpfile.clone(), infile.clone()])?.truthy() {
                    error_(it, &self_, s("Error renaming temporary file after deleting original"))?;
                    rtn_val = n(0);
                }
            }
            // the output file should now have the name of the original infile
            if pl::gt(&rtn_val, &n(0)) {
                outfile = infile.clone();
            }
        }
    }
    // set filesystem attributes if requested (and if possible!)
    if pl::gt(&rtn_val, &n(0)) && (close_out || (outbuff_defined(&out_buff) && (close_in || isa_(it, &infile, "GLOB")?))) {
        let target = if close_out { outfile.clone() } else { infile.clone() };
        // set file permissions if requested
        if pl::gt(&system_tags(it, &self_, &target)?, &n(0)) {
            pre_inc(&self_, "CHANGED");
        }
        if close_in {
            // (no use setting file times unless the input file is closed)
            if set_mod_date && pl::gt(&file_modify_date(it, vec![self_.clone(), target.clone(), original_time.clone(), Value::Undef, n(1)])?, &n(0)) {
                pre_inc(&self_, "CHANGED");
            }
            // set FileCreateDate if requested (and if possible!)
            if set_create_date && pl::gt(&file_modify_date(it, vec![self_.clone(), target.clone(), create_time.clone(), s("FileCreateDate"), n(1)])?, &n(0)) {
                pre_inc(&self_, "CHANGED");
            }
            // create hard link if requested and no output filename specified (and if possible!)
            if !hard_link.is_undef() && file_name(it, vec![self_.clone(), target.clone(), hard_link.clone(), s("HardLink")])?.truthy() {
                pre_inc(&self_, "CHANGED");
            }
            if !sym_link.is_undef() && file_name(it, vec![self_.clone(), target.clone(), sym_link.clone(), s("SymLink")])?.truthy() {
                pre_inc(&self_, "CHANGED");
            }
            if !test_name.is_undef() {
                file_name(it, vec![self_.clone(), target.clone(), test_name.clone(), s("Test")])?;
            }
        }
    }
    // check for write error and set appropriate error message and return value
    if pl::lt(&rtn_val, &n(0)) {
        if !error_v(&self_).truthy() {
            error_(it, &self_, s("Error writing output file"))?;
        }
        rtn_val = n(0); // return 0 on failure
    } else if pl::gt(&rtn_val, &n(0)) && !self_.get("CHANGED").truthy() {
        rtn_val = pl::add(&rtn_val, &n(1));
    }
    // set things back to the way they were
    self_.set("RAF", old_raf);

    ret(rtn_val)
}

pub fn register(it: &mut Interp) {
    it.define_native("Image::ExifTool::SetNewValue", set_new_value);
    it.define_native("Image::ExifTool::SetNewValuesFromFile", set_new_values_from_file);
    it.define_native("Image::ExifTool::SetFileModifyDate", set_file_modify_date);
    it.define_native("Image::ExifTool::SetFileName", set_file_name);
    it.define_native("Image::ExifTool::SetSystemTags", set_system_tags);
    it.define_native("Image::ExifTool::WriteInfo", write_info);
    it.define_native("Image::ExifTool::InsertTagValues", insert_tag_values);
    it.define_native("Image::ExifTool::Get64s", get64s);
    it.define_native("Image::ExifTool::Get64u", get64u);
    it.define_native("Image::ExifTool::GetFixed64s", get_fixed64s);
    it.define_native("Image::ExifTool::GetExtended", get_extended);
    it.define_native("Image::ExifTool::IsRawType", is_raw_type);
    it.define_native("Image::ExifTool::GetNewFileName", get_new_file_name);
    it.define_native("Image::ExifTool::NextFreeTagKey", next_free_tag_key);
    it.define_native("Image::ExifTool::GetWriteGroup", get_write_group);
    it.define_native("Image::ExifTool::GetWriteGroup1", get_write_group1);
    it.define_native("Image::ExifTool::CountNewValues", count_new_values);
    it.define_native("Image::ExifTool::SaveNewValues", save_new_values);
    it.define_native("Image::ExifTool::RestoreNewValues", restore_new_values);
    it.define_native("Image::ExifTool::SetAlternateFile", set_alternate_file);
    it.define_native("Image::ExifTool::GetNewGroups", get_new_groups);
    it.define_native("Image::ExifTool::GetDeleteGroups", get_delete_groups);
    it.define_native("Image::ExifTool::AddUserDefinedTags", add_user_defined_tags);
    it.define_native("Image::ExifTool::GetNewValues", get_new_values);
    it.define_native("Image::ExifTool::Sanitize", sanitize);
    it.define_native("Image::ExifTool::PushValue", push_value);
    it.define_native("Image::ExifTool::DateFmt", date_fmt);
    it.define_native("Image::ExifTool::NoDups", no_dups);
    it.define_native("Image::ExifTool::SetTags", set_tags);
    it.define_native("Image::ExifTool::IsWritable", is_writable);
    it.define_native("Image::ExifTool::IsSameFile", is_same_file);
    it.define_native("Image::ExifTool::CopyFileAttrs", copy_file_attrs);
    it.define_native("Image::ExifTool::ReverseLookup", reverse_lookup);
    it.define_native("Image::ExifTool::GetGeolocateTags", get_geolocate_tags);
    it.define_native("Image::ExifTool::GetNewValueHash", get_new_value_hash);
    it.define_native("Image::ExifTool::LoadAllTables", load_all_tables);
    it.define_native("Image::ExifTool::RemoveNewValueHash", remove_new_value_hash);
    it.define_native("Image::ExifTool::RemoveNewValuesForGroup", remove_new_values_for_group);
    it.define_native("Image::ExifTool::GetNewTagInfoList", get_new_tag_info_list);
    it.define_native("Image::ExifTool::GetNewTagInfoHash", get_new_tag_info_hash);
    it.define_native("Image::ExifTool::GetAddDirHash", get_add_dir_hash);
    it.define_native("Image::ExifTool::GetLangInfo", get_lang_info);
    it.define_native("Image::ExifTool::GetAllTags", get_all_tags);
    it.define_native("Image::ExifTool::GetWritableTags", get_writable_tags);
    it.define_native("Image::ExifTool::GetAllGroups", get_all_groups);
    it.define_native("Image::ExifTool::GetNewValue", get_new_value);
    it.define_native("Image::ExifTool::IsOverwriting", is_overwriting);
    it.define_native("Image::ExifTool::ConvInv", conv_inv);
    it.define_native("Image::ExifTool::InitWriteDirs", init_write_dirs);
    it.define_native("Image::ExifTool::WriteDirectory", write_directory);
}
