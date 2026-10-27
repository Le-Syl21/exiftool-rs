//! Image::ExifTool::Plot (Image/ExifTool/Plot.pm), ported sub by sub (see docs/port/PORTING.md).

use std::rc::Rc;

use crate::interp::eval::{die, ER};
use crate::interp::{Interp, Scope, Want};
use crate::perl::Value;
use crate::pl::{self, add, arg, cap, concat, div, ge, gt, int, le, lt, mul, n, re, ret, sub};
use crate::runtime;

const MODULE: &str = "Image::ExifTool::Plot";

// ---- local helpers ----

fn s(x: &str) -> Value {
    Value::str(x)
}

fn f(x: f64) -> Value {
    Value::Num(x)
}

/// `$$h{$k}` with a value as the key.
fn hget(h: &Value, k: &Value) -> Value {
    h.get(&*k.to_bytes())
}

/// `$$h{$k} = $v` with a value as the key.
fn hset(h: &Value, k: &Value, v: Value) {
    h.set(&*k.to_bytes(), v);
}

/// String interpolation: the values concatenated.
fn cat(parts: &[Value]) -> Value {
    let mut b = Vec::new();
    for p in parts {
        b.extend_from_slice(&p.to_bytes());
    }
    Value::bytes(b)
}

/// `STR x COUNT`
fn rep(st: &str, count: &Value) -> Value {
    Value::bytes(st.as_bytes().repeat(count.to_i64().max(0) as usize))
}

/// `print $fp LIST`
fn pr(it: &mut Interp, fp: &Value, items: &[Value]) -> ER<bool> {
    pl::print_to(it, Some(fp), items)
}

/// `$$a[$i] = $v` with perl's negative indices.
fn set_idx(a: &Value, i: &Value, v: Value) -> ER<()> {
    let mut i = i.to_i64();
    if i < 0 {
        i += a.len() as i64;
        if i < 0 {
            return die(format!("Modification of non-creatable array value attempted, subscript {}", i - a.len() as i64));
        }
    }
    a.set_at(i as usize, v);
    Ok(())
}

/// Run a perl builtin the interpreter implements (`srand`, `rand`), so the
/// port draws the same numbers as the interpreted module.
fn perl_expr(it: &mut Interp, code: &str) -> ER<Value> {
    let scope = Scope::new(None);
    match it.eval_string(code.as_bytes(), MODULE, &scope, Want::Scalar) {
        Ok(v) => Ok(v.last().cloned().unwrap_or_default()),
        Err(e) => Err(crate::interp::Flow::Die(e)),
    }
}

/// `%defaults` (a file lexical).
fn defaults(it: &mut Interp) -> Value {
    runtime::lex_hash(it, MODULE, "defaults")
}

/// Shallow copy of a value as `{ %$h }` / `[ @$a ]` do.
fn copy_container(v: &Value) -> Value {
    match pl::ref_of(v).as_str() {
        "HASH" => v.hash_copy(),
        "ARRAY" => Value::new_array(v.items()),
        _ => v.clone(),
    }
}

//------------------------------------------------------------------------------
// Create new plot object
pub fn new(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let that = arg(&a, 0);
    let mut class = if that.is_ref() { s(&pl::ref_of(&that)) } else { that.clone() };
    if !class.truthy() {
        class = s("Image::ExifTool::Plot");
    }
    let self_ = Value::new_hash();
    if let Value::HashRef(rc) = &self_ {
        rc.borrow_mut().blessed = Some(Rc::from(class.to_text().as_str()));
    }
    let d = defaults(it);
    for k in d.keys() {
        let v = d.get(&*k);
        self_.set(&*k, copy_container(&v));
    }
    ret(self_)
}

//------------------------------------------------------------------------------
// Set plot settings
// Inputs: 0) Plot ref, 1) comma-separated options
pub fn settings(it: &mut Interp, a: Vec<Value>, want: Want) -> ER<Vec<Value>> {
    let (self_, set) = (arg(&a, 0), arg(&a, 1));
    if !set.truthy() {
        return pl::ret_nothing(want);
    }
    for item in pl::split(&re(r",\s*", ""), &set, 0) {
        let Some(c) = re(r"^([a-z].*?)(=(.*))?$", "i").m(&item) else { continue };
        let (name, mut val) = (pl::lc(&cap(&c, 1)), cap(&c, 3));
        let cur = hget(&self_, &name);
        if pl::ref_of(&cur) == "ARRAY" {
            if val.is_undef() {
                continue;
            }
            let is_num = re(r"^\d+$", "").is_match(&cur.at(0));
            // also allow numbers to also be separated by 'x'
            let mut vals = if is_num { pl::split(&re(r"\s*[x\s\/+]\s*", ""), &val, 0) } else { pl::split(&re(r"\s*[\s\/+]\s*", ""), &val, 0) };
            let mut i: i64 = 0;
            while !vals.is_empty() {
                let mut val = pl::lc(&vals.remove(0));
                if val.to_bytes().is_empty() {
                    i += 1;
                    continue;
                }
                if pl::str_eq(&name, "marks") {
                    let mut v = pl::split(&re("-", ""), &val, 0);
                    if v.first().is_some_and(|x| x.truthy()) {
                        if re("^n", "").is_match(&v[0]) {
                            v[0] = s("none");
                        } else {
                            let mut p = b"^".to_vec();
                            p.extend_from_slice(&v[0].to_bytes());
                            let r = pl::re_dyn(&p, "")?;
                            let dm = defaults(it).get("marks");
                            v[0] = dm.items().into_iter().find(|m| r.is_match(m)).unwrap_or_default();
                            if !v[0].truthy() {
                                self_.set("Warn", s("Invalid marker name"));
                                i += 1;
                                continue;
                            }
                        }
                    } else {
                        // cycle through default markers if none specified
                        let dm = defaults(it).get("marks");
                        let m = dm.at(pl::modulo(&n(i), &n(dm.len() as i64))?.to_i64());
                        if v.is_empty() {
                            v.push(m);
                        } else {
                            v[0] = m;
                        }
                    }
                    val = pl::join("-", &v);
                }
                cur.set_at(i as usize, val);
                i += 1;
            }
        } else {
            if val.is_undef() {
                val = n(1); // default to 1 if no "="
            }
            // escape necessary XML characters, but allow numerical entities
            let did = re(r"([&><])", "s").subst(&mut val, true, |c| {
                let nm: &[u8] = match &*cap(c, 1).to_bytes() {
                    b"&" => b"amp",
                    b"<" => b"lt",
                    _ => b"gt",
                };
                [b"&".as_slice(), nm, b";"].concat()
            });
            if did > 0 {
                re(r"&amp;(#(\d+|x[0-9a-fA-F]+);)", "").subst(&mut val, false, |c| [b"&".as_slice(), &cap(c, 1).to_bytes()].concat());
            }
            if val.to_bytes().is_empty() {
                val = Value::Undef;
            }
            hset(&self_, &name, val);
        }
    }
    pl::ret_nothing(want)
}

/// `shift @$a` on an array reference.
fn shift_ref(a: &Value) -> Value {
    match a {
        Value::ArrayRef(rc) => {
            let mut v = rc.borrow_mut();
            if v.is_empty() {
                Value::Undef
            } else {
                v.remove(0)
            }
        }
        _ => Value::Undef,
    }
}

//------------------------------------------------------------------------------
// Add points to SVG plot
// Inputs: 0) Plot object ref, 1) tag value hash ref, 2) tag ID list ref
pub fn add_points(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let _ = it;
    let (self_, info, tags) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    let (mut tag, mut name) = (Value::Undef, Value::Undef);
    let num = Value::new_hash();
    let (mut index, mut mod_) = (Value::Undef, Value::Undef);
    let mut val;
    let mut vals: Vec<Value> = Vec::new();
    let (ee, doc_num, data, mut xmin, mut xmax) = (self_.get("EE"), self_.get("DocNum"), self_.get("Data"), self_.get("XMin"), self_.get("XMax"));
    if !self_.get("type").truthy() {
        self_.set("type", s("line"));
    }
    let scat = if re("^s", "").is_match(&self_.get("type")) { 1 } else { 0 };
    let mut xname = self_.get("Name").at(0); // (x-axis name if using scatter plot)
    let max_lines = if re("^h", "").is_match(&self_.get("type")) && !self_.get("multi").truthy() { 1 } else { 20 };
    let num_re = re(r"^[+-]?(?=\.?\d)\d*\.?\d*(?:e[+-]?\d+)?$", "");
    let num_sep_re = re(r"^[+-]?(?=\.?\d)\d*\.?\d*(?:e[+-]?\d+)?([ ,;\t\n\r]+|$)", "i");
    let split_re = re(r"[ ,;\t\n\r][\n\r]? *", "");
    loop {
        if !vals.is_empty() {
            val = vals.remove(0);
            if !num_re.is_match(&val) {
                continue;
            }
        } else {
            tag = shift_ref(&tags);
            if !tag.truthy() {
                break;
            }
            // ignore non-floating-point values
            val = hget(&info, &tag);
            name = re(r"^(\S+)", "").all(&tag).into_iter().next().unwrap_or_default(); // remove index number
            if val.is_ref() {
                let r = pl::ref_of(&val);
                if r == "ARRAY" {
                    index = if !index.is_undef() { add(&index, &n(1)) } else { n(0) };
                    val = val.at(index.to_i64());
                    if val.is_undef() {
                        index = Value::Undef;
                        mod_ = Value::Undef;
                        continue;
                    }
                    name = if mod_.truthy() { cat(&[name.clone(), s("["), pl::modulo(&index, &mod_)?, s("]")]) } else { cat(&[name.clone(), s("["), index.clone(), s("]")]) };
                    tags.unshift(tag.clone()); // will continue with this tag later
                } else if r == "SCALAR" {
                    val = val.deref(); // handle binary values
                } else {
                    continue;
                }
            }
        }
        if val.is_undef() {
            continue;
        }
        let Some(c) = num_sep_re.m(&val) else { continue };
        if cap(&c, 1).truthy() {
            // split a string of numbers into separate plot points (eg. histogram tags)
            if self_.get("split").truthy() {
                // make an array
                hset(&info, &tag, Value::new_array(pl::split(&split_re, &val, 0)));
                tags.unshift(tag.clone());
                // split into lists of 'split' elements if split > 1
                if gt(&self_.get("split"), &n(1)) {
                    mod_ = self_.get("split");
                }
                continue;
            } else {
                vals = pl::split(&split_re, &val, 0);
                val = if vals.is_empty() { Value::Undef } else { vals.remove(0) };
            }
        }
        let doc_num = if doc_num.truthy() {
            let d = hget(&doc_num, &tag);
            if d.truthy() {
                d
            } else {
                n(0)
            }
        } else {
            n(0)
        };
        if doc_num.truthy() && !ee.truthy() {
            continue;
        }
        if !hget(&data, &name).truthy() {
            if (self_.get("Name").len() as i64) >= max_lines + scat {
                if !self_.get("MaxTags").truthy() {
                    if re("^h", "").is_match(&self_.get("type")) && !self_.get("multi").truthy() {
                        self_.set("Warn", s("Use the Multi setting to make a separate histogram for each dataset"));
                    } else {
                        self_.set("Warn", s("Too many variables to plot all of them"));
                    }
                    self_.set("MaxTags", n(1));
                }
                continue;
            }
            self_.get_or_new_array("Name").push(name.clone());
            if !xname.truthy() {
                xname = name.clone(); // x-axis data for scatter plot
            }
            if !(scat != 0 && name.to_bytes() == xname.to_bytes()) {
                if self_.get("Max").is_undef() || gt(&val, &self_.get("Max")) {
                    self_.set("Max", val.clone());
                }
                if self_.get("Min").is_undef() || lt(&val, &self_.get("Min")) {
                    self_.set("Min", val.clone());
                }
            }
            if xmin.is_undef() {
                xmax = doc_num.clone();
                xmin = doc_num.clone();
            }
            hset(&num, &name, xmax.clone());
            if ge(&xmax, &xmin) {
                let arr = data.get_or_new_array(&*name.to_bytes());
                set_idx(&arr, &sub(&xmax, &xmin), val.clone())?;
            }
            continue;
        }
        if doc_num.truthy() && !hget(&num, &name).is_undef() && lt(&hget(&num, &name), &doc_num) {
            hset(&num, &name, doc_num.clone()); // keep documents synchronized if some tags are missing
        } else {
            if hget(&num, &name).is_undef() {
                hset(&num, &name, xmax.clone());
            }
            hset(&num, &name, add(&hget(&num, &name), &n(1)));
        }
        if ge(&hget(&num, &name), &xmin) {
            let arr = data.get_or_new_array(&*name.to_bytes());
            set_idx(&arr, &sub(&hget(&num, &name), &xmin), val.clone())?;
        }
        if !(scat != 0 && name.to_bytes() == xname.to_bytes()) {
            if gt(&val, &self_.get("Max")) {
                self_.set("Max", val.clone());
            }
            if lt(&val, &self_.get("Min")) {
                self_.set("Min", val.clone());
            }
        }
    }
    // start next file at x value so far
    for k in num.keys() {
        let v = num.get(&*k);
        if lt(&xmax, &v) {
            xmax = v;
        }
    }
    self_.set("XMin", xmin);
    self_.set("XMax", xmax.clone());
    ret(xmax)
}

//------------------------------------------------------------------------------
// Calculate a nice round number for grid spacing
// Inputs: 0) nominal spacing (must be positive), 1) flag to increment to next number
// Returns: spacing rounded to an even number
pub fn get_grid_spacing(_it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    ret(grid_spacing(&arg(&a, 0), &arg(&a, 1)))
}

fn grid_spacing(nom: &Value, inc: &Value) -> Value {
    let mut div_ = pl::sprintf("%.3e", &[nom.clone()]);
    let mut num = pl::substr(&div_, 0, Some(1));
    let mut exp = if re(".*e", "").subst(&mut div_, false, |_| Vec::new()) > 0 { div_.clone() } else { n(0) };
    if inc.truthy() {
        // increment to next highest even number
        num = if lt(&num, &n(2)) {
            n(2)
        } else if lt(&num, &n(5)) {
            n(5)
        } else {
            exp = add(&exp, &n(1));
            n(1)
        };
    } else {
        // look for nearest factor to 1, 2 or 5 * 10^x
        if gt(&num, &n(2)) {
            num = if lt(&num, &n(8)) {
                n(5)
            } else {
                exp = add(&exp, &n(1));
                n(1)
            };
        }
    }
    if ge(&exp, &n(0)) {
        concat(&num, &rep("0", &exp))
    } else {
        cat(&[s("."), rep("0", &sub(&pl::neg(&exp), &n(1))), num])
    }
}

//------------------------------------------------------------------------------
// Get plot range
// Inputs: 0) minimum, 1) maximum
// Returns: difference
// Notes: Adjusts min/max if necessary to make difference positive
pub fn get_range(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (mut a0, mut a1) = (arg(&a, 0), arg(&a, 1));
    let changed = ge(&a0, &a1);
    let r = range(&mut a0, &mut a1)?;
    if changed {
        it.set_arg(0, a0)?;
        it.set_arg(1, a1)?;
    }
    ret(r)
}

/// GetRange on the caller's two variables.
fn range(a0: &mut Value, a1: &mut Value) -> ER<Value> {
    if ge(a0, a1) {
        *a0 = div(&add(a0, a1), &n(2))?;
        if a0.truthy() {
            *a0 = sub(a0, &f(0.5));
        }
        *a1 = add(a0, &n(1));
    }
    Ok(sub(a1, a0))
}

/// `int($x/$dy) ...` rounding helpers of Draw.
fn round_down(x: &Value, d: &Value) -> ER<Value> {
    // ($x > 0 ? int($x/$d) : int($x/$d-0.9999)) * $d
    let q = div(x, d)?;
    Ok(mul(&if gt(x, &n(0)) { int(&q) } else { int(&sub(&q, &f(0.9999))) }, d))
}

fn round_up(x: &Value, d: &Value) -> ER<Value> {
    // ($x > 0 ? int($x/$d+0.9999) : int($x/$d)) * $d
    let q = div(x, d)?;
    Ok(mul(&if gt(x, &n(0)) { int(&add(&q, &f(0.9999))) } else { int(&q) }, d))
}

/// `int(($x) * 10 + 0.5) / 10`
fn round1(x: &Value) -> ER<Value> {
    div(&int(&add(&mul(x, &n(10)), &f(0.5))), &n(10))
}

//------------------------------------------------------------------------------
// Draw SVG plot
// Inputs: 0) Plot ref, 1) Output file reference
pub fn draw(it: &mut Interp, a: Vec<Value>, want: Want) -> ER<Vec<Value>> {
    let (self_, fp) = (arg(&a, 0), arg(&a, 1));
    let (mut min, mut max, mut xmin, mut xmax, mut name, mut style) = (self_.get("Min"), self_.get("Max"), self_.get("XMin"), self_.get("XMax"), self_.get("Name"), self_.get("style"));
    let mut multi_multi = false;
    let defaults = defaults(it);
    let ng = runtime::lex_array(it, MODULE, "ng");
    let wch = runtime::lex_scalar(it, MODULE, "wch").borrow().clone();
    let marker_data = runtime::lex_hash(it, MODULE, "markerData");

    if min.is_undef() || xmin.is_undef() {
        self_.set("Error", s("Nothing to plot"));
        return pl::ret_nothing(want);
    }
    let type_ = self_.get("type");
    let scat = if re("^s", "").is_match(&type_) { 1usize } else { 0 };
    let hist = if re("^h", "").is_match(&type_) { Value::new_array(vec![]) } else { n(0) };
    let mut multi = self_.get("multi");
    if !multi.truthy() {
        multi = n(0);
    }
    let mut multi_l = re(r"\d+", "").all(&multi);
    let mut names = name.items();
    if scat != 0 && !names.is_empty() {
        names.remove(0);
    }
    multi = if multi_l.is_empty() { Value::Undef } else { multi_l.remove(0) };
    if !gt(&multi, &n(0)) {
        multi = n(0);
    }
    if !style.truthy() {
        style = s(if hist.truthy() { "line+fill" } else { "line" });
    }
    if !(re(r"\b[mpl]", "").is_match(&style) || (hist.truthy() && re(r"\bf", "").is_match(&style))) {
        self_.set("Error", s("Invalid plot Style setting"));
        return pl::ret_nothing(want);
    }
    let ml = |i: usize| -> Value { multi_l.get(i).cloned().unwrap_or_default() };
    let mut num_plots: usize = 0;
    if multi.truthy() {
        let mut nn = n(0);
        while lt(&nn, &n(name.len() as i64 - scat as i64)) {
            let m = ml(num_plots);
            nn = add(&nn, &if m.truthy() { m.clone() } else { n(1) });
            if m.truthy() && gt(&m, &n(1)) {
                multi_multi = true;
            }
            num_plots += 1;
        }
    } else {
        num_plots = 1;
    }
    let mut size = self_.get("size").items();
    let mut sy = size.get(1).cloned().unwrap_or_default();
    if multi.truthy() {
        sy = mul(&sy, &div(&int(&div(&sub(&add(&n(num_plots as i64), &multi), &n(1)), &multi)?), &multi)?);
        for v in size.iter_mut() {
            *v = div(v, &multi)?;
        }
    }
    let sz = |i: usize| -> Value { size.get(i).cloned().unwrap_or_default() };
    let mut tmp = self_.get("title");
    if !tmp.truthy() {
        tmp = concat(&s("Plot by ExifTool "), &it.get_scalar("Image::ExifTool::VERSION"));
    }
    let s0 = self_.get("size").at(0);
    pr(it, &fp, &[cat(&[
        s("<?xml version=\"1.0\" standalone=\"no\"?>\n<!DOCTYPE svg PUBLIC \"-//W3C//DTD SVG 20010904//EN\" \"http://www.w3.org/TR/2001/REC-SVG-20010904/DTD/svg10.dtd\">\n<svg version=\"1.1\" xmlns=\"http://www.w3.org/2000/svg\" width=\""),
        s0.clone(),
        s("\" height=\""),
        sy.clone(),
        s("\"\n preserveAspectRatio=\"xMidYMid meet\" viewBox=\"0 0 "),
        s0.clone(),
        s(" "),
        sy.clone(),
        s("\">\n<title>"),
        tmp,
        s("</title>"),
    ])])?;
    // loop through all plots
    for plot_num in 0..num_plots {
        let mut i: i64;
        let mut nn;
        let col = Value::new_hash();
        let class = Value::new_hash();
        let (dx, mut dy): (Value, Value);
        let (mut dx2, mut x_axis) = (Value::Undef, false);
        let (mut x, mut y, mut px, mut py);
        let mut og: Vec<Value> = Vec::new();
        let mut no_legend = false;
        let mut xname = Value::Undef;
        let mut xdat = Value::Undef;
        let (mut xdiff, mut diff);
        let mark_id = Value::new_hash();
        if num_plots > 1 {
            let tx = mul(&pl::modulo(&n(plot_num as i64), &multi)?, &sz(0));
            let ty = mul(&int(&div(&n(plot_num as i64), &multi)?), &sz(1));
            pr(it, &fp, &[s("\n<g transform='translate("), tx, s(","), ty, s(")'>")])?;
            let keys = ["XMin", "XMax", "title", "xlabel", "ylabel"];
            if plot_num != 0 {
                let saved = self_.get("Save");
                for (j, k) in keys.iter().enumerate() {
                    self_.set(*k, saved.at(j as i64));
                }
            } else {
                self_.set("Save", Value::new_array(keys.iter().map(|k| self_.get(*k)).collect()));
                self_.set("SaveName", Value::new_array(self_.get("Name").items()));
            }
            name = Value::new_array(vec![]);
            self_.set("Name", name.clone());
            if scat != 0 {
                name.push(self_.get("SaveName").at(0));
            }
            let m = ml(plot_num);
            let cnt = if m.truthy() { m.to_i64() } else { 1 };
            for _ in 0..cnt.max(0) {
                name.push(if names.is_empty() { Value::Undef } else { names.remove(0) });
            }
            min = Value::Undef;
            max = Value::Undef;
            for k in scat..name.len() {
                let dat = hget(&self_.get("Data"), &name.at(k as i64));
                for v in dat.items() {
                    if v.is_undef() {
                        continue;
                    }
                    if min.is_undef() {
                        min = v.clone();
                        max = v.clone();
                        continue;
                    }
                    if gt(&min, &v) {
                        min = v.clone();
                    }
                    if lt(&max, &v) {
                        max = v.clone();
                    }
                }
            }
        }
        let (data, mut title, mut xlabel, mut ylabel, cols, marks, tpad, wid) = (self_.get("Data"), self_.get("title"), self_.get("xlabel"), self_.get("ylabel"), self_.get("cols"), self_.get("marks"), self_.get("txtpad"), self_.get("stroke"));
        let mut name_l = name.items();
        let mut margin = self_.get("margin").items();
        while margin.len() < 4 {
            margin.push(Value::Undef);
        }

        // set reasonable default titles and labels
        if scat != 0 {
            xname = if name_l.is_empty() { Value::Undef } else { name_l.remove(0) };
        }
        let nl0 = || name_l.first().cloned().unwrap_or_default();
        if scat != 0 && !title.is_undef() && !title.truthy() && name_l.len() == 1 && !multi.truthy() {
            title = cat(&[nl0(), s(" vs "), xname.clone()]);
        }
        if (scat != 0 || hist.truthy()) && !xlabel.is_undef() && !xlabel.truthy() {
            xlabel = name.at(0);
            if hist.truthy() {
                no_legend = true;
            }
        }
        if !ylabel.is_undef() && !ylabel.truthy() && name_l.len() == 1 && !multi_multi {
            ylabel = if hist.truthy() { s("Count") } else { nl0() };
            if !hist.truthy() {
                no_legend = true;
            }
        }

        // make room for title/labels
        let ls = self_.get("linespacing");
        if title.truthy() {
            margin[1] = add(&margin[1], &mul(&ls, &f(1.5)));
        }
        if xlabel.truthy() {
            margin[3] = add(&margin[3], &ls);
        }
        if ylabel.truthy() {
            margin[0] = add(&margin[0], &ls);
        }

        // calculate optimal number of X/Y grid lines
        let dsize = defaults.get("size");
        let dmargin = defaults.get("margin");
        for i in 0..2usize {
            let num_ = mul(&ng.at(i as i64), &sub(&sub(&sz(i), &margin[i]), &margin[i + 2]));
            let den = sub(&sub(&dsize.at(i as i64), &dmargin.at(i as i64)), &dmargin.at(i as i64 + 2));
            og.push(div(&num_, &den)?);
            if le(&og[i], &n(0)) {
                self_.set("Error", s("Invalid plot size"));
                return pl::ret_nothing(want);
            }
        }
        if scat != 0 {
            xdat = hget(&self_.get("Data"), &xname);
            if !(!self_.get("xmin").is_undef() && !self_.get("xmax").is_undef()) {
                let mut set = false;
                for v in xdat.items() {
                    if v.is_undef() {
                        continue;
                    }
                    if !set {
                        xmin = v.clone();
                        xmax = v.clone();
                        set = true;
                        continue;
                    }
                    if gt(&xmin, &v) {
                        xmin = v.clone();
                    }
                    if lt(&xmax, &v) {
                        xmax = v.clone();
                    }
                }
                let dnx2 = div(&sub(&xmax, &xmin), &mul(&og[0], &n(2)))?;
                // leave a bit of a left/right margin, but don't pass 0
                xmin = if ge(&xmin, &n(0)) && lt(&xmin, &dnx2) { n(0) } else { sub(&xmin, &dnx2) };
                xmax = if le(&xmax, &n(0)) && lt(&pl::neg(&xmax), &dnx2) { n(0) } else { add(&xmax, &dnx2) };
            }
            if !self_.get("xmin").is_undef() {
                xmin = self_.get("xmin");
            }
            if !self_.get("xmax").is_undef() {
                xmax = self_.get("xmax");
            }
        } else {
            // shift x range to correspond with index in data list
            xmax = sub(&xmax, &xmin);
            xmin = n(0);
        }
        if hist.truthy() {
            if !gt(&self_.get("nbins"), &n(0)) {
                self_.set("Error", s("Invalid number of histogram bins"));
                return pl::ret_nothing(want);
            }
            no_legend = true;
            // y axis becomes histogram x axis after binning
            if !self_.get("xmin").is_undef() {
                min = self_.get("xmin");
            }
            if !self_.get("xmax").is_undef() {
                max = self_.get("xmax");
            }
        } else {
            // leave a bit of a margin above/below data when autoscaling but don't pass 0
            let dny2 = div(&sub(&max, &min), &mul(&og[1], &n(2)))?;
            min = if ge(&min, &n(0)) && lt(&min, &dny2) { n(0) } else { sub(&min, &dny2) };
            max = if le(&max, &n(0)) && lt(&pl::neg(&max), &dny2) { n(0) } else { add(&max, &dny2) };
            // adjust to user-defined range if specified
            if !self_.get("ymin").is_undef() {
                min = self_.get("ymin");
            }
            if !self_.get("ymax").is_undef() {
                max = self_.get("ymax");
            }
        }
        // generate random colors if we need more
        while cols.len() < name.len() {
            if !self_.get("seeded").truthy() {
                perl_expr(it, "srand(141)")?;
                self_.set("seeded", n(1));
            }
            let r = perl_expr(it, "int(rand(220))")?;
            let g = perl_expr(it, "int(rand(220))")?;
            let b = perl_expr(it, "int(rand(220))")?;
            cols.push(pl::sprintf("#%.2x%.2x%.2x", &[r, g, b]));
        }
        diff = range(&mut min, &mut max)?;
        xdiff = range(&mut xmin, &mut xmax)?;

        // determine y grid spacing (nice even numbers)
        dy = grid_spacing(&div(&diff, &if hist.truthy() { self_.get("nbins") } else { og[1].clone() })?, &Value::Undef);
        // expand plot min/max to the nearest even multiple of our grid spacing
        min = round_down(&min, &dy)?;
        max = round_up(&max, &dy)?;

        // bin histogram
        if hist.truthy() {
            let dat = hget(&data, &name_l.first().cloned().unwrap_or_default());
            let nmax = int(&add(&div(&sub(&max, &min), &dy)?, &f(0.5)));
            if let Value::ArrayRef(rc) = &hist {
                *rc.borrow_mut() = vec![n(0); nmax.to_i64().max(0) as usize];
            }
            for v in dat.items() {
                if v.is_undef() {
                    continue;
                }
                nn = div(&sub(&v, &min), &dy)?;
                if lt(&nn, &n(0)) || gt(&nn, &add(&nmax, &f(0.00001))) {
                    continue;
                }
                nn = int(&nn);
                let idx = if lt(&nn, &nmax) { nn.clone() } else { sub(&nmax, &n(1)) };
                let cur = {
                    let ii = idx.to_i64();
                    if ii < 0 {
                        let jj = ii + hist.len() as i64;
                        if jj < 0 {
                            return die(format!("Modification of non-creatable array value attempted, subscript {ii}"));
                        }
                        hist.at(jj)
                    } else {
                        hist.at(ii)
                    }
                };
                set_idx(&hist, &idx, add(&cur, &n(1)))?;
            }
            xmin = min.clone();
            xmax = max.clone();
            min = n(0);
            max = n(0);
            if self_.get("ymax").truthy() {
                max = self_.get("ymax");
            } else {
                for v in hist.items() {
                    if lt(&max, &v) {
                        max = v;
                    }
                }
            }
            diff = range(&mut min, &mut max)?;
            dx = dy.clone();
            dy = grid_spacing(&div(&diff, &og[1])?, &Value::Undef);
            max = round_up(&max, &dy)?;
            hset(&data, &name_l.first().cloned().unwrap_or_default(), hist.clone());
        } else {
            dx = grid_spacing(&div(&xdiff, &og[0])?, &Value::Undef);
        }
        if scat != 0 {
            xmin = round_down(&xmin, &dx)?;
            xmax = round_up(&xmax, &dx)?;
        }
        diff = range(&mut min, &mut max)?;
        xdiff = range(&mut xmin, &mut xmax)?;
        // width/height of plot area
        let width = sub(&sub(&sz(0), &margin[0]), &margin[2]);
        let height = sub(&sub(&sz(1), &margin[1]), &margin[3]);
        let yscl = div(&height, &diff)?;
        let mut xscl = div(&width, &xdiff)?;
        let mut px0 = sub(&margin[0], &mul(&xmin, &xscl));
        let py0 = add(&add(&margin[1], &height), &mul(&min, &yscl));
        let clip = [sub(&margin[0], &mul(&n(6), &wid)), sub(&margin[1], &mul(&n(6), &wid)), add(&width, &mul(&n(12), &wid)), add(&height, &mul(&n(12), &wid))];
        pr(it, &fp, &[s("\n<!-- Definitions -->\n<defs>\n<clipPath id='plot-area'>")])?;
        pr(it, &fp, &[cat(&[s("<rect x='"), clip[0].clone(), s("' y='"), clip[1].clone(), s("' width='"), clip[2].clone(), s("' height='"), clip[3].clone(), s("'/></clipPath>")])])?;
        let dmarks = defaults.get("marks");
        if re(r"\b[mp]", "").is_match(&style) {
            // 'm' for 'marker' or 'p' for 'point' (undocumented)
            for i in 0..name_l.len() as i64 {
                let mi = marks.at(i);
                let msrc = if mi.truthy() { mi } else { dmarks.at(pl::modulo(&n(i), &n(dmarks.len() as i64))?.to_i64()) };
                let m = pl::split(&re("-", ""), &msrc, 0);
                let mm = |k: usize| m.get(k).cloned().unwrap_or_default();
                let mut fill = Value::Undef;
                let cond = if mm(1).truthy() { re("^f", "").is_match(&mm(1)) } else { re(r"\bf", "").is_match(&style) };
                if cond {
                    fill = if mm(2).truthy() { mm(2) } else { cols.at(i) };
                }
                let mut mark = hget(&marker_data, &mm(0));
                if !mark.truthy() {
                    hset(&mark_id, &mark, s(""));
                    continue; // skip 'none' or unrecognized marker name
                }
                if fill.truthy() && !pl::str_eq(&fill, "none") {
                    let op = if mm(3).truthy() { mm(3) } else if pl::str_eq(&cols.at(i), "none") { n(50) } else { n(20) };
                    mark = cat(&[mark, s(" fill=\""), fill.clone(), s("\" style=\"fill-opacity: "), op, s("%\"")]);
                } else {
                    mark = concat(&mark, &s(" fill=\"none\""));
                }
                // (was using 'context-stroke', but Chrome didn't show this properly)
                mark = cat(&[mark, s(" stroke='"), cols.at(i), s("'/>")]);
                // don't re-define mark if it is the same as a previous one
                if hget(&mark_id, &mark).truthy() {
                    hset(&mark_id, &n(i), hget(&mark_id, &mark));
                    continue;
                }
                let id = s(&format!("mark{i}"));
                hset(&mark_id, &mark, id.clone());
                hset(&mark_id, &n(i), id.clone());
                pr(it, &fp, &[cat(&[s("\n<marker id='"), id, s("' markerWidth='8' markerHeight='8' refX='4'")]), cat(&[s(" refY='4'>\n"), mark, s("\n</marker>")])])?;
            }
            pr(it, &fp, &[s("\n</defs>\n<style>")])?;
            for i in 0..name_l.len() as i64 {
                if &*hget(&mark_id, &n(i)).to_bytes() != format!("mark{i}").as_bytes() {
                    continue;
                }
                pr(it, &fp, &[s(&format!("\n  path.mark{i} {{ marker: url(#mark{i}) }}"))])?;
            }
        } else {
            pr(it, &fp, &[s("\n</defs>\n<style>")])?;
        }
        pr(it, &fp, &[cat(&[s("\n  text { fill: "), self_.get("text"), s(" }\n</style>")])])?;
        if self_.get("bkg").truthy() {
            pr(it, &fp, &[cat(&[s("\n<rect x='0' y='0' width='"), sz(0), s("' height='"), sz(1), s("' fill='"), self_.get("bkg"), s("'/>")])])?;
        }
        pr(it, &fp, &[s("\n<!-- X axis -->")])?;
        pr(it, &fp, &[s("\n<g dominant-baseline='hanging' text-anchor='middle'>")])?;
        py = round1(&add(&add(&margin[1], &height), &tpad.at(1)))?;
        px = round1(&add(&margin[0], &div(&width, &n(2))?))?;
        if title.truthy() {
            pr(it, &fp, &[cat(&[s("\n<text x='"), px.clone(), s("' y='14' font-size='150%'>"), title.clone(), s("</text>")])])?;
        }
        if xlabel.truthy() {
            y = add(&py, &self_.get("linespacing"));
            pr(it, &fp, &[cat(&[s("\n<text x='"), px.clone(), s("' y='"), y.clone(), s("'>"), xlabel.clone(), s("</text>")])])?;
        }
        if ylabel.truthy() {
            y = add(&margin[1], &div(&height, &n(2))?);
            pr(it, &fp, &[cat(&[s("\n<text x='10' y='"), y.clone(), s("' transform='rotate(-90,10,"), y.clone(), s(")'>"), ylabel.clone(), s("</text>")])])?;
        }
        // make sure the X labels will fit
        let mut spc = dx.clone();
        loop {
            // find longest label at current spacing
            let mut len = 0i64;
            let x0 = mul(&int(&add(&div(&xmax, &spc)?, &f(0.5))), &spc); // get value of last x label
            x = x0;
            for _ in 0..3 {
                let nlen = pl::sprintf("%g", &[x.clone()]).to_bytes().len() as i64;
                if len < nlen {
                    len = nlen;
                }
                x = sub(&x, &spc);
            }
            if ge(&spc, &div(&mul(&mul(&n(len + 1), &wch), &xdiff), &width)?) {
                break;
            }
            // increase label spacing by one increment and try again
            dx2 = grid_spacing(&spc, &n(1));
            spc = dx2.clone();
        }
        let (mut grid, mut last_len) = (s(""), 0usize);
        x = mul(&int(&sub(&div(&xmin, &dx)?, &n(1))), &dx);
        loop {
            px = round1(&add(&margin[0], &div(&mul(&sub(&x, &xmin), &width), &xdiff)?))?;
            if lt(&px, &sub(&margin[0], &f(0.5))) {
                x = add(&x, &dx);
                continue;
            }
            if gt(&px, &add(&add(&margin[0], &width), &f(0.5))) {
                break;
            }
            let mut h = height.clone();
            let show = !dx2.truthy() || {
                let q = div(&x, &dx2)?;
                let rr = int(&add(&q, &if gt(&x, &n(0)) { f(0.5) } else { f(-0.5) }));
                lt(&pl::abs(&sub(&q, &rr)), &f(0.01))
            };
            if show {
                let fmt = cat(&[s("\n<text x='"), px.clone(), s("' y='"), py.clone(), s("'>%g</text>")]);
                pr(it, &fp, &[pl::sprintf_v(&fmt, &[x.clone()])])?;
                h = add(&h, &div(&tpad.at(1), &n(2))?);
            }
            if grid.to_bytes().len() as i64 - last_len as i64 > 80 {
                grid = concat(&grid, &s("\n"));
                last_len = grid.to_bytes().len();
            }
            let fmt = cat(&[s("M"), px.clone(), s(" "), margin[1].clone(), s(" v"), h, s(" ")]);
            grid = concat(&grid, &pl::sprintf_v(&fmt, &[]));
            x = add(&x, &dx);
        }
        pr(it, &fp, &[cat(&[s("\n<path stroke='"), self_.get("grid"), s("' stroke-width='0.5' d='\n"), grid.clone(), s("'/>")])])?;
        pr(it, &fp, &[s("\n</g>\n<!-- Y axis -->\n<g dominant-baseline='middle' text-anchor='end'>")])?;
        px = round1(&sub(&margin[0], &tpad.at(0)))?;
        grid = s("");
        last_len = 0;
        let (gx, gw) = (sub(&margin[0], &div(&tpad.at(0), &n(2))?), add(&width, &div(&tpad.at(0), &n(2))?));
        y = min.clone();
        loop {
            py = round1(&sub(&add(&margin[1], &height), &mul(&sub(&y, &min), &yscl)))?;
            if lt(&py, &sub(&margin[1], &f(0.5))) {
                break;
            }
            let half = div(&dy, &n(2))?;
            if lt(&y, &half) && gt(&y, &pl::neg(&half)) {
                y = n(0); // (avoid round-off errors)
            }
            let fmt = cat(&[s("\n<text x='"), px.clone(), s("' y='"), py.clone(), s("'>%g</text>")]);
            pr(it, &fp, &[pl::sprintf_v(&fmt, &[y.clone()])])?;
            if lt(&y, &half) && gt(&y, &pl::neg(&half)) {
                x_axis = true; // redraw x axis later
            }
            if grid.to_bytes().len() as i64 - last_len as i64 > 80 {
                grid = concat(&grid, &s("\n"));
                last_len = grid.to_bytes().len();
            }
            grid = concat(&grid, &cat(&[s("M"), gx.clone(), s(" "), py.clone(), s(" h"), gw.clone(), s(" ")]));
            y = add(&y, &dy);
        }
        if x_axis && !pl::num_eq(&min, &n(0)) {
            py = add(&add(&margin[1], &height), &mul(&min, &yscl));
            pr(it, &fp, &[cat(&[s("\n<path stroke='"), self_.get("text"), s("' d='M"), margin[0].clone(), s(" "), py.clone(), s(" h"), width.clone(), s("'/>")])])?;
        }
        pr(it, &fp, &[cat(&[s("\n<path stroke='"), self_.get("grid"), s("' stroke-width='0.5' d='\n"), grid.clone(), s("'/>")])])?;
        pr(it, &fp, &[s("\n</g>\n<!-- Plot box and legend -->\n<g dominant-baseline='middle' text-anchor='start'>")])?;
        pr(it, &fp, &[cat(&[s("\n<path stroke='"), self_.get("text"), s("' fill='none' d='M"), margin[0].clone(), s(" "), margin[1].clone(), s(" l0 "), height.clone(), s(" "), width.clone(), s(" 0 0 -"), height.clone(), s(" z'/>")])])?;
        i = 0;
        while (i as usize) < name_l.len() && !no_legend {
            x = add(&sub(&sub(&sz(0), &margin[2]), &n(175)), &self_.get("legend").at(0));
            y = add(&add(&add(&margin[1], &self_.get("legend").at(1)), &n(15)), &mul(&self_.get("linespacing"), &add(&n(i), &f(0.5))));
            let colv = cols.at(i);
            let mid = hget(&mark_id, &n(i));
            let mark = if mid.truthy() { cat(&[s(" marker-end='url(#"), mid.clone(), s(")' fill='none'")]) } else { s("") };
            let line = if re(r"\bl", "").is_match(&style) { s(" l-20 0") } else { pl::sprintf(" m%.4g 0", &[mul(&n(-5), &wid)]) };
            let sw = mul(&if re(r"\bm", "").is_match(&style) { f(1.5) } else { n(2) }, &wid); // (wider for line-only style so colour is more visible)
            pr(it, &fp, &[cat(&[s("\n<path"), mark, s(" stroke-width='"), sw, s("' stroke='"), colv, s("' d='M"), x.clone(), s(" "), y.clone(), s(" m-7 -1"), line, s("'/>")])])?;
            pr(it, &fp, &[cat(&[s("\n<text x='"), x.clone(), s("' y='"), y.clone(), s("'>"), name_l[i as usize].clone(), s("</text>")])])?;
            i += 1;
        }
        // print the data
        for (k, nm) in name_l.iter().enumerate() {
            hset(&col, nm, cols.at(k as i64));
            let mid = hget(&mark_id, &n(k as i64));
            hset(&class, nm, if mid.truthy() { cat(&[s(" class='"), mid, s("'")]) } else { s("") });
        }
        let (i0, i1): (i64, i64);
        let mut xsclr = Value::Undef;
        let mut fill = s("");
        if scat != 0 {
            i0 = 0;
            i1 = xdat.len() as i64 - 1;
        } else if hist.truthy() {
            i0 = 0;
            i1 = hist.len() as i64 - 1;
            xscl = div(&width, &n(hist.len() as i64))?;
            px0 = margin[0].clone();
            xsclr = div(&int(&add(&mul(&xscl, &n(100)), &f(0.5))), &n(100))?;
            if re(r"\bf", "").is_match(&style) {
                let m = pl::split(&re("-", ""), &marks.at(0), 0);
                let m3 = m.get(3).cloned().unwrap_or_default();
                let op = if m3.truthy() { m3 } else if re(r"\bl", "").is_match(&style) { n(20) } else { n(50) };
                fill = cat(&[s(" fill='"), cols.at(0), s("'")]);
                if !pl::str_eq(&cols.at(0), "none") {
                    fill = cat(&[fill, s(" style='fill-opacity: "), op, s("%'")]);
                }
            }
        } else {
            let mut a0 = int(&xmin).to_i64() - 1;
            if a0 < 0 {
                a0 = 0;
            }
            i0 = a0;
            i1 = int(&xmax).to_i64() + 1;
        }
        pr(it, &fp, &[s("\n</g>\n<!-- Datasets -->\n<g fill='none' clip-path='url(#plot-area)'"), s(" stroke-linejoin='round' stroke-linecap='round' stroke-width='"), mul(&f(1.5), &wid), s("'>")])?;
        let do_lines = re(r"\bl", "").is_match(&style);
        for nm in name_l.iter() {
            let stroke = if hist.truthy() && !do_lines { s("none") } else { hget(&col, nm) };
            let dat = hget(&data, nm);
            pr(it, &fp, &[cat(&[s("\n<!-- "), nm.clone(), s(" -->")])])?;
            pr(it, &fp, &[cat(&[s("\n<path"), hget(&class, nm), fill.clone(), s(" stroke='"), stroke, s("' d='")])])?;
            if do_lines {
                pr(it, &fp, &[s("M")])?;
            }
            let mut m = s(if do_lines { "" } else { " M" });
            let mut i = i0;
            while i <= i1 {
                let di = dat.at(i);
                if di.is_undef() {
                    i += 1;
                    continue;
                }
                y = round1(&sub(&py0, &mul(&di, &yscl)))?;
                if scat != 0 {
                    let xi = xdat.at(i);
                    if xi.is_undef() {
                        i += 1;
                        continue;
                    }
                    x = round1(&add(&px0, &mul(&xi, &xscl)))?;
                } else {
                    x = round1(&add(&px0, &mul(&n(i), &xscl)))?;
                    if hist.truthy() {
                        pr(it, &fp, &[m.clone(), s(if i % 5 != 0 { " " } else { "\n" }), cat(&[x.clone(), s(" "), y.clone(), s(" h"), xsclr.clone()])])?;
                        m = s(" L"); // (draw lines after the first point)
                        i += 1;
                        continue;
                    }
                }
                pr(it, &fp, &[m.clone(), s(if i % 10 != 0 { " " } else { "\n" }), cat(&[x.clone(), s(" "), y.clone()])])?;
                i += 1;
            }
            if hist.truthy() && fill.truthy() {
                pr(it, &fp, &[s(" V"), add(&margin[1], &height), cat(&[s(" H"), margin[0].clone(), s(" z")])])?;
            }
            pr(it, &fp, &[s("'/>")])?;
        }
        pr(it, &fp, &[s("\n</g>")])?;
        if num_plots > 1 {
            pr(it, &fp, &[s("\n</g>")])?;
        }
    }
    let ok = pr(it, &fp, &[s("</svg>\n")])?;
    if !ok {
        self_.set("Error", s("Error writing output plot file"));
        return ret(s("Error writing output plot file"));
    }
    ret(n(1))
}

pub fn register(it: &mut Interp) {
    it.define_native("Image::ExifTool::Plot::new", new);
    it.define_native("Image::ExifTool::Plot::Settings", settings);
    it.define_native("Image::ExifTool::Plot::AddPoints", add_points);
    it.define_native("Image::ExifTool::Plot::GetGridSpacing", get_grid_spacing);
    it.define_native("Image::ExifTool::Plot::GetRange", get_range);
    it.define_native("Image::ExifTool::Plot::Draw", draw);
}
