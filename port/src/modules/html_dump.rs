//! Image::ExifTool::HtmlDump (Image/ExifTool/HtmlDump.pm), ported sub by sub (see docs/port/PORTING.md).

use crate::interp::eval::ER;
use crate::interp::{Interp, Scope, Want};
use crate::perl::{ops, Value};
use crate::pl::{self, arg, cap, n, re, ret};
use crate::runtime;

const MODULE: &str = "Image::ExifTool::HtmlDump";

// ---- local helpers ----

fn s(x: &str) -> Value {
    Value::str(x)
}

/// `$a & $b`
fn band(a: &Value, b: i64) -> Value {
    ops::bit_and(a, &n(b))
}

/// `$$h{$k}` with a value as the key.
fn hget(h: &Value, k: &Value) -> Value {
    h.get(&*k.to_bytes())
}

/// `$$a[$i] .= $v`
fn append_at(a: &Value, i: usize, v: &[u8]) {
    if let Value::ArrayRef(rc) = a {
        let mut arr = rc.borrow_mut();
        if i >= arr.len() {
            arr.resize(i + 1, Value::Undef);
        }
        let mut b = arr[i].to_bytes().to_vec();
        b.extend_from_slice(v);
        arr[i] = Value::bytes(b);
    }
}

/// `STR x COUNT` (the count truncated to an integer; nothing when negative).
fn rep(s: &str, count: &Value) -> String {
    let c = count.to_f64();
    if c >= 1.0 {
        s.repeat(c as usize)
    } else {
        String::new()
    }
}

/// `++$id` on a one-letter class name ('V' becomes 'W'): perl's magic
/// string increment, for the letters DumpTable uses.
fn incr_id(id: &Value) -> Value {
    let mut b = id.to_bytes().to_vec();
    match b.last_mut() {
        Some(c) if c.is_ascii_alphabetic() && *c != b'z' && *c != b'Z' => *c += 1,
        _ => return pl::add(id, &n(1)),
    }
    Value::bytes(b)
}

/// A file lexical scalar of HtmlDump.pm (`my $name`).
fn my_scalar(it: &mut Interp, name: &str) -> Value {
    runtime::lex_scalar(it, MODULE, name).borrow().clone()
}

fn set_my_scalar(it: &mut Interp, name: &str, v: Value) {
    *runtime::lex_scalar(it, MODULE, name).borrow_mut() = v;
}

/// `@bkgSpan` (a file lexical).
fn bkg_span(it: &mut Interp) -> Value {
    runtime::lex_array(it, MODULE, "bkgSpan")
}

//------------------------------------------------------------------------------
// New - create new HtmlDump object
// Inputs: 0) reference to HtmlDump object or HtmlDump class name
pub fn new(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    pl::local_topic(it, |_it| {
        let that = arg(&a, 0);
        let mut class = if that.is_ref() { s(&pl::ref_of(&that)) } else { that.clone() };
        if !class.truthy() {
            class = s("Image::ExifTool::HtmlDump");
        }
        let h = Value::new_hash();
        h.set("Block", Value::new_hash());
        h.set("TipNum", n(0));
        if let Value::HashRef(rc) = &h {
            rc.borrow_mut().blessed = Some(Rc::from(class.to_text().as_str()));
        }
        ret(h)
    })
}

use std::rc::Rc;

//------------------------------------------------------------------------------
// Add information to dump
// Inputs: 0) HTML dump hash ref, 1) absolute offset in file, 2) data size,
//         3) comment string, 4) tool tip (or SAME to use previous tip),
//         5) bit flags (see below), 6) IFD name
// Bits: 0x01 - print at start of line
//       0x02 - print red address
//       0x04 - maker notes data ('M'-class span)
//       0x08 - limit block length
//       0x10 - allow double references
//       0x100 - (reserved)
// Notes: Block will be shown in 'unused' color if comment string begins with '['
pub fn add(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (self_, start, size, msg, mut tip, flag, ifd) = (arg(&a, 0), arg(&a, 1), arg(&a, 2), arg(&a, 3), arg(&a, 4), arg(&a, 5), arg(&a, 6));
    let block = self_.get("Block");
    if !hget(&block, &start).truthy() {
        block.set(&*start.to_bytes(), Value::new_array(vec![]));
    }
    let mut htip;
    if tip.truthy() && pl::str_eq(&tip, "SAME") {
        htip = s("");
    } else {
        // use message as first line of tip, and make bold unless in brackets
        htip = if re(r"^[\[(]", "").is_match(&msg) { msg.clone() } else { pl::concat(&pl::concat(&s("<b>"), &msg), &s("</b>")) };
        if !tip.is_undef() {
            tip = pl::call(it, "Image::ExifTool::HTML::EscapeHTML", vec![tip.clone()])?;
            re(r"\n", "").subst(&mut tip, true, |_| b"<br>".to_vec()); // HTML-ize tooltip text
            htip = pl::concat(&pl::concat(&htip, &s("<br>")), &tip);
        }
        // add size if not already done
        if !re(r"<br>Size:", "").is_match(&htip) {
            htip = pl::concat(&htip, &pl::concat(&pl::concat(&s("<br>("), &size), &s(" bytes)")));
        }
        self_.set("TipNum", pl::add(&self_.get("TipNum"), &n(1)));
    }
    let entry = Value::new_array(vec![size, msg, htip, flag, self_.get("TipNum"), ifd]);
    hget(&block, &start).push(entry.clone());
    ret(n(hget(&block, &start).len() as i64))
}

//------------------------------------------------------------------------------
// Print dump information to HTML page
// Inputs: 0) Dump information hash reference, 1) source file RAF reference,
//         2) data pointer, 3) data position, 4) output file or scalar reference,
//         5) limit level (1-3), 6) title
// Returns: non-zero if useful output was generated,
//          or -1 on error loading data and "ERROR" is set to offending data name
// Note: The "Error" member may be set externally to print a specific error
//       message instead of doing the dump.
pub fn print(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    pl::local_topic(it, |it| print_body(it, a))
}

fn print_body(it: &mut Interp, a: Vec<Value>) -> ER<Vec<Value>> {
    let (self_, raf, data_pt, mut data_pos, mut outfile, mut level, mut title) = (arg(&a, 0), arg(&a, 1), arg(&a, 2), arg(&a, 3), arg(&a, 4), arg(&a, 5), arg(&a, 6));
    let mut buff = Value::Undef;
    let mut err = Value::Undef;
    let rtn_val;
    let limit;
    let block = self_.get("Block");
    if !data_pos.truthy() {
        data_pos = n(0);
    }
    if !outfile.is_ref() {
        let scope = Scope::new(None);
        outfile = it.eval_string(b"\\*STDOUT", "main", &scope, Want::Scalar).map_err(crate::interp::Flow::Die)?.pop().unwrap_or_default();
    }
    if !title.truthy() {
        title = s("HtmlDump");
    }
    if !level.truthy() {
        level = n(0);
    }
    let tell = pl::method(it, &raf, "Tell", vec![])?;
    if !pl::method(it, &raf, "Seek", vec![n(0), n(2)])?.truthy() {
        self_.set("ERROR", s("Seek error"));
        return ret(n(-1));
    }
    let file_len = pl::method(it, &raf, "Tell", vec![])?;
    let mut pos = n(0);
    let data_end = pl::add(&data_pos, &if data_pt.truthy() { n(data_pt.deref().to_bytes().len() as i64) } else { n(0) });
    // initialize member variables
    self_.set("Open", Value::new_array(vec![]));
    self_.set("Closed", Value::new_array(vec![]));
    self_.set("TipList", Value::new_array(vec![]));
    self_.set("MSpanList", Value::new_array(vec![]));
    self_.set("Cols", Value::new_array(vec![s(""), s(""), s(""), s("")])); // text columns
    // set dump size limits (limits are 4x smaller if bit 0x08 set in flags)
    if pl::le(&level, &n(1)) {
        limit = n(1024);
    } else if pl::le(&level, &n(2)) {
        limit = n(16384);
    } else {
        limit = n(256 * 1024 * 1024); // never dump bigger than 256 MB
    }
    self_.set("Limit", limit.clone());
    // pre-initialize open/closed hashes for all columns
    for i in 0..4 {
        let o = Value::new_hash();
        o.set("ID", Value::new_array(vec![]));
        o.set("Element", Value::new_hash());
        self_.get("Open").set_at(i, o);
        let c = Value::new_hash();
        c.set("ID", Value::new_array(vec![]));
        c.set("Element", Value::new_hash());
        self_.get("Closed").set_at(i, c);
    }
    set_my_scalar(it, "bkgStart", n(0));
    set_my_scalar(it, "bkgEnd", n(0));
    if let Value::ArrayRef(rc) = bkg_span(it) {
        rc.borrow_mut().clear();
    }
    let mut index = n(0); // initialize tooltip index
    let names = Value::new_array(vec![]);
    let mut was_unused = Value::Undef;
    let mut starts: Vec<Value> = Vec::new();
    // only do dump if we didn't have a serious error
    if !self_.get("Error").truthy() {
        starts = block.keys().into_iter().map(|k| Value::bytes(&k[..])).collect();
        starts.sort_by(|a, b| ops::num_cmp(a, b).unwrap_or(std::cmp::Ordering::Equal));
    }
    let mut i: i64 = 0;
    while i <= starts.len() as i64 {
        let mut start = if i >= 0 { starts.get(i as usize).cloned().unwrap_or_default() } else { starts.get((starts.len() as i64 + i) as usize).cloned().unwrap_or_default() };
        let mut parm_list;
        let bkg_end = my_scalar(it, "bkgEnd");
        if !start.is_undef() {
            parm_list = hget(&block, &start);
        } else if bkg_end.truthy() && pl::lt(&pos, &bkg_end) && was_unused.is_undef() {
            start = bkg_end.clone(); // finish last bkg block
            parm_list = Value::Undef;
        } else {
            break;
        }
        if pl::gt(&start, &file_len) {
            start = file_len.clone(); // handle case of bad start offset
        }
        let mut len = pl::sub(&start, &pos);
        if pl::gt(&len, &n(0)) && !was_unused.truthy() {
            // we have a unused bytes before this data block
            i -= 1; // dump the data block next time around
            // split unused data into 2 blocks if it spans end of a bkg block
            let mut next_bkg_end = Value::Undef;
            if was_unused.is_undef() && bkg_end.truthy() {
                for bkg in bkg_span(it).items() {
                    let be = pl::add(&bkg.get("End"), &data_pos);
                    if pl::ge(&pos, &be) || pl::le(&pl::add(&pos, &len), &be) {
                        continue;
                    }
                    if !(next_bkg_end.truthy() && pl::lt(&next_bkg_end, &bkg.get("End"))) {
                        next_bkg_end = bkg.get("End");
                    }
                }
            }
            if next_bkg_end.truthy() {
                start = pos.clone();
                len = pl::sub(&pl::add(&next_bkg_end, &data_pos), &pos);
                was_unused = n(0);
            } else {
                start = pos.clone(); // dump the unused bytes now
                was_unused = n(1); // avoid re-dumping unused bytes if we get a read error
            }
            let str_ = if pl::gt(&len, &n(1)) { format!("unused {} bytes", len.to_text()) } else { "pad byte".to_string() };
            parm_list = Value::new_array(vec![Value::new_array(vec![len.clone(), s(&format!("[{str_}]")), Value::Undef, n(0x108)])]);
        } else {
            was_unused = Value::Undef;
        }
        let plist = parm_list.items();
        for parms in plist.iter() {
            let p = parms.items();
            let (mut len, msg, mut tip, mut flag, tip_num, ifd) = (arg(&p, 0), arg(&p, 1), arg(&p, 2), arg(&p, 3), arg(&p, 4), arg(&p, 5));
            if !pl::gt(&len, &n(0)) {
                continue;
            }
            if flag.is_undef() {
                flag = n(0);
            }
            // generate same name for all blocks indexed by this tooltip
            let mut name = Value::Undef;
            if !tip_num.is_undef() {
                name = names.at(tip_num.to_i64());
            }
            let mut idx = index.clone();
            if name.truthy() {
                // get index from existing ID
                idx = pl::substr(&name, 1, None);
            } else {
                name = pl::concat(&s("t"), &index);
                if !tip_num.is_undef() {
                    names.set_at(tip_num.to_i64() as usize, name.clone());
                }
                index = pl::add(&index, &n(1));
            }
            if band(&flag, 0x14).truthy() {
                let mut class = if band(&flag, 0x04).truthy() { pl::concat(&name, &s(" M")) } else { name.clone() };
                if ifd.truthy() {
                    class = pl::concat(&pl::concat(&class, &s(" ")), &ifd);
                }
                let bkg = Value::new_hash();
                bkg.set("Class", class);
                bkg.set("Start", pl::sub(&start, &data_pos));
                bkg.set("End", pl::add(&pl::sub(&start, &data_pos), &len));
                bkg_span(it).push(bkg.clone());
                let bkg_start = my_scalar(it, "bkgStart");
                if !(bkg_start.truthy() && pl::lt(&bkg_start, &bkg.get("Start"))) {
                    set_my_scalar(it, "bkgStart", bkg.get("Start"));
                }
                let bkg_end = my_scalar(it, "bkgEnd");
                if !(bkg_end.truthy() && pl::gt(&bkg_end, &bkg.get("End"))) {
                    set_my_scalar(it, "bkgEnd", bkg.get("End"));
                }
                self_.get("MSpanList").push(name.clone());
                continue;
            }
            // loop until we read the value properly
            let mut end = Value::Undef;
            for _try in 0..2 {
                end = pl::add(&start, &len);
                // only load as much of the block as we are going to dump
                // (read 32 more bytes than necessary just in case there
                // is only one skipped line that we decide to print)
                let size = if pl::gt(&len, &pl::add(&limit, &n(32))) { pl::add(&pl::div(&limit, &n(2))?, &n(16)) } else { len.clone() };
                if pl::ge(&start, &data_pos) && pl::le(&end, &data_end) {
                    let data = data_pt.deref();
                    buff = pl::substr(&data, pl::sub(&start, &data_pos).to_i64(), Some(size.to_i64()));
                    if !pl::num_eq(&len, &size) {
                        let off = pl::sub(&pl::add(&pl::sub(&start, &data_pos), &len), &size);
                        buff = pl::concat(&buff, &pl::substr(&data, off.to_i64(), Some(size.to_i64())));
                    }
                } else {
                    buff = s("");
                    let ok = pl::method(it, &raf, "Seek", vec![start.clone(), n(0)])?.truthy() && {
                        let mut args = [buff.clone(), size.clone()];
                        let r = pl::method_mut(it, &raf, "Read", &mut args, Want::Scalar)?.pop().unwrap_or_default();
                        buff = args[0].clone();
                        pl::num_eq(&r, &size)
                    };
                    if ok {
                        // read end of block
                        if !pl::num_eq(&len, &size) {
                            let mut buf2 = s("");
                            let ok2 = pl::method(it, &raf, "Seek", vec![pl::sub(&pl::add(&start, &len), &size), n(0)])?.truthy() && {
                                let mut args = [buf2.clone(), size.clone()];
                                let r = pl::method_mut(it, &raf, "Read", &mut args, Want::Scalar)?.pop().unwrap_or_default();
                                buf2 = args[0].clone();
                                pl::num_eq(&r, &size)
                            };
                            if !ok2 {
                                err = msg.clone();
                                // reset $len to the actual length of available data
                                len = pl::sub(&file_len, &start);
                                if tip.truthy() {
                                    tip = pl::concat(&tip, &s(&format!("<br>Error: Only {} bytes available!", len.to_text())));
                                }
                                continue;
                            }
                            buff = pl::concat(&buff, &buf2);
                        }
                    } else {
                        err = msg.clone();
                        len = n(buff.to_bytes().len() as i64);
                        if tip.truthy() {
                            tip = pl::concat(&tip, &s(&format!("<br>Error: Only {} bytes available!", len.to_text())));
                        }
                    }
                }
                break;
            }
            if tip.truthy() {
                self_.get("TipList").set_at(idx.to_i64() as usize, tip.clone());
            }
            if buff.to_bytes().is_empty() {
                continue;
            }
            // set flag to continue this line if next block is contiguous
            if i + 1 < starts.len() as i64 && plist.last().is_some_and(|l| l.same_ref(parms)) {
                let next_start = &starts[(i + 1) as usize];
                if pl::num_eq(&end, next_start) || (pl::lt(&end, next_start) && pl::ge(&end, &pos)) {
                    let mut next_flag = hget(&block, next_start).at(0).at(3);
                    if !next_flag.truthy() {
                        next_flag = n(0);
                    }
                    if !(band(&flag, 0x01).truthy() || band(&next_flag, 0x01).truthy()) {
                        flag = ops::bit_or(&flag, &n(0x100));
                    }
                }
            }
            dump_table(it, vec![self_.clone(), pl::sub(&start, &data_pos), pl::sref(buff.clone()), msg.clone(), name.clone(), flag.clone(), len.clone(), pl::sub(&pos, &data_pos), ifd.clone()], Want::Void)?;
            buff = Value::Undef;
            if pl::lt(&pos, &end) {
                pos = end.clone();
            }
        }
        i += 1;
    }
    open(it, vec![self_.clone(), s(""), s("")], Want::Void)?; // close all open elements
    pl::method(it, &raf, "Seek", vec![tell, n(0)])?;

    // write output HTML file
    let header1 = my_scalar(it, "htmlHeader1");
    let header2 = my_scalar(it, "htmlHeader2");
    let header3 = my_scalar(it, "htmlHeader3");
    let pre_mouse = my_scalar(it, "preMouse");
    write(it, vec![outfile.clone(), header1, title.clone()], Want::Void)?;
    let cols = self_.get("Cols");
    if cols.at(0).truthy() {
        write(it, vec![outfile.clone(), header2], Want::Void)?;
        let mspan = self_.get("MSpanList");
        for (i, m) in mspan.items().iter().enumerate() {
            write(it, vec![outfile.clone(), s(&format!("mspan[{i}] = \"{}\";\n", m.to_text()))], Want::Void)?;
        }
        write(it, vec![outfile.clone(), header3, cols.at(0)], Want::Void)?;
        write(it, vec![outfile.clone(), s("</pre></td><td valign=\"top\">"), pre_mouse.clone(), cols.at(1)], Want::Void)?;
        write(it, vec![outfile.clone(), s("</pre></td><td class=c2 valign=\"top\">"), pre_mouse.clone(), cols.at(2)], Want::Void)?;
        write(it, vec![outfile.clone(), s("</pre></td><td valign=\"top\">"), pre_mouse.clone(), cols.at(3)], Want::Void)?;
        write(it, vec![outfile.clone(), s("</pre></td></tr></table>\n<div id=tb class=tb> </div>\n")], Want::Void)?;
        let tips = self_.get("TipList");
        for (i, tip) in tips.items().iter().enumerate() {
            if !tip.is_undef() {
                write(it, vec![outfile.clone(), pl::concat(&pl::concat(&s(&format!("<div id=p{i} class=tt>")), tip), &s("</div>\n"))], Want::Void)?;
            }
        }
        self_.delete("TipList");
        rtn_val = n(1);
    } else {
        let mut e = self_.get("Error");
        if !e.truthy() {
            e = s("No EXIF or TIFF information found in image");
        }
        let m = pl::concat(&pl::concat(&pl::concat(&title, &s("</title></head><body>\n")), &e), &s("\n"));
        write(it, vec![outfile.clone(), m], Want::Void)?;
        rtn_val = n(0);
    }
    write(it, vec![outfile.clone(), s("</body></html>\n")], Want::Void)?;
    for i in 0..4 {
        self_.get("Cols").set_at(i, s("")); // free memory
    }
    if err.truthy() {
        pl::tr(&mut err, "()", "", "d");
        self_.set("ERROR", err);
        return ret(n(-1));
    }
    ret(rtn_val)
}

//------------------------------------------------------------------------------
// Open or close a specified html element
// Inputs: 0) HtmlDump object ref, 1) element id, 2) element string,
//         3-N) list of column numbers (empty for all columns)
// - element id may be '' to close all elements
// - element string may be '' to close element by ID (or 0 to close without reopening)
// - element id and string may both be 1 to reopen temporarily closed elements
pub fn open(_it: &mut Interp, a: Vec<Value>, want: Want) -> ER<Vec<Value>> {
    let (self_, id, element) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    let mut col_nums: Vec<Value> = a.iter().skip(3).cloned().collect();

    // loop through specified columns
    if col_nums.is_empty() {
        col_nums = (0..self_.get("Open").len() as i64).map(n).collect();
    }
    let idk = id.to_bytes();
    for col in col_nums {
        let ci = col.to_i64();
        // get information about open elements in this column
        let op_hash = self_.get("Open").at(ci);
        let op_elem = op_hash.get("Element");
        if element.truthy() {
            // next if already open
            let cur = op_elem.get(&*idk);
            if cur.truthy() && cur.to_bytes() == element.to_bytes() {
                continue;
            }
        } else if id.truthy() && !op_elem.get(&*idk).truthy() {
            // next if already closed and nothing to reopen
            if !(pl::str_eq(&element, "") && !self_.get("Closed").at(ci).get("ID").is_empty()) {
                continue;
            }
        }
        let op_id = op_hash.get("ID");
        let cl_hash = self_.get("Closed").at(ci);
        let mut cl_id = cl_hash.get("ID");
        let mut cl_elem = cl_hash.get("Element");
        // get reference to output column list (use temp list if available)
        let mut cols = self_.get("TmpCols");
        if !cols.truthy() {
            cols = self_.get("Cols");
        }
        let cu = ci as usize;
        // close everything down to this element if necessary
        if op_elem.get(&*idk).truthy() || !id.truthy() {
            loop {
                let tid = match &op_id {
                    Value::ArrayRef(rc) => rc.borrow_mut().pop(),
                    _ => None,
                };
                let Some(tid) = tid else { break };
                let tk = tid.to_bytes();
                let mut e = op_elem.get(&*tk);
                re(r"^<(\S+).*", "s").subst(&mut e, false, |c| [b"</".as_slice(), &cap(c, 1).to_bytes(), b">"].concat());
                append_at(&cols, cu, &e.to_bytes());
                if id.to_bytes() == tk || !id.truthy() {
                    op_elem.delete(&*tk);
                    if id.truthy() {
                        break;
                    }
                    continue;
                }
                // add this to the temporarily closed list
                // (because we really didn't want to close it)
                cl_id.push(tid.clone());
                cl_elem.set(&*tk, op_elem.get(&*tk));
                op_elem.delete(&*tk);
            }
            if !id.truthy() {
                // forget all temporarily closed elements
                cl_id = Value::new_array(vec![]);
                cl_hash.set("ID", cl_id.clone());
                cl_elem = Value::new_hash();
                cl_hash.set("Element", cl_elem.clone());
            }
        } else if cl_elem.get(&*idk).truthy() {
            // delete from the list of temporarily closed elements
            cl_elem.delete(&*idk);
            let mut p = b"^".to_vec();
            p.extend_from_slice(&idk);
            p.push(b'$');
            let r = pl::re_dyn(&p, "")?;
            let kept: Vec<Value> = cl_id.items().into_iter().filter(|v| !r.is_match(v)).collect();
            if let Value::ArrayRef(rc) = &cl_id {
                *rc.borrow_mut() = kept;
            }
        }
        if pl::str_eq(&element, "0") {
            continue; // 0 = don't reopen temporarily closed elements
        }

        // re-open temporarily closed elements
        loop {
            let tid = match &cl_id {
                Value::ArrayRef(rc) => rc.borrow_mut().pop(),
                _ => None,
            };
            let Some(tid) = tid else { break };
            let tk = tid.to_bytes();
            append_at(&cols, cu, &cl_elem.get(&*tk).to_bytes());
            op_id.push(tid.clone());
            op_elem.set(&*tk, cl_elem.get(&*tk));
            cl_elem.delete(&*tk);
        }
        // open specified element
        if element.truthy() && !pl::str_eq(&element, "1") {
            append_at(&cols, cu, &element.to_bytes());
            op_id.push(id.clone());
            op_elem.set(&*idk, element.clone());
        }
    }
    pl::ret_nothing(want)
}

/// `$self->Open(ID, ELEMENT, COLS...)` with string arguments.
fn open_(it: &mut Interp, self_: &Value, id: &str, element: Value, cols: &[i64]) -> ER<()> {
    let mut a = vec![self_.clone(), s(id), element];
    a.extend(cols.iter().map(|&c| n(c)));
    open(it, a, Want::Void)?;
    Ok(())
}

//------------------------------------------------------------------------------
// Dump a block of data in HTML table form
// Inputs: 0) HtmlDump object ref, 1) data position, 2) block pointer,
//         3) message, 4) object name, 5) flag, 6) full block length (actual
//         data may be shorter), 7) data end position, 8) IFD name
pub fn dump_table(it: &mut Interp, a: Vec<Value>, want: Want) -> ER<Vec<Value>> {
    let (self_, pos, block_pt, mut msg, mut name, flag, mut len, mut end_pos, ifd) = (arg(&a, 0), arg(&a, 1), arg(&a, 2), arg(&a, 3), arg(&a, 4), arg(&a, 5), arg(&a, 6), arg(&a, 7), arg(&a, 8));
    let block = block_pt.deref();
    let blen = block.to_bytes().len() as i64;
    if len.is_undef() {
        len = n(blen);
    }
    if !end_pos.truthy() {
        end_pos = n(0);
    }
    let mut f0;
    let mut dbl_ref = false;
    let mut id = Value::Undef;
    let mut skipped = n(0);
    if (end_pos.truthy() && pl::lt(&pos, &end_pos)) || band(&flag, 0x02).truthy() {
        // display double-reference addresses in red
        f0 = s("<span class=V>");
        if end_pos.truthy() && pl::lt(&pos, &end_pos) {
            dbl_ref = true;
        }
    } else {
        f0 = s("");
    }
    let c = Value::new_array(vec![s(""), s(""), s(""), s("")]);
    self_.set("TmpCols", c.clone());
    let ifd_text = ifd.to_text();
    if name.truthy() {
        if msg.truthy() && re(r"^\[", "").is_match(&msg) {
            id = s("U");
        } else {
            if self_.get("A").truthy() {
                id = s("X");
                self_.set("A", n(0));
            } else {
                id = s("V");
                self_.set("A", n(1));
            }
            if !dbl_ref {
                id = incr_id(&id);
            }
        }
        let class = if ifd.truthy() { format!("'{} {}'", id.to_text(), ifd_text) } else { id.to_text() };
        name = s(&format!("<a name={} class={}>", name.to_text(), class));
        if msg.truthy() {
            msg = pl::concat(&pl::concat(&name, &msg), &s("</a>"));
        }
    } else {
        name = s("");
    }
    // use base-relative offsets from now on
    let mut cols = n(0);
    let mut p = pos.clone();
    if self_.get("Cont").truthy() {
        cols = band(&pos, 0x0f);
        append_at(&c, 1, if pl::num_eq(&cols, &n(8)) { b"  " } else { b" " });
    } else {
        let addr = if pl::lt(&pos, &n(0)) { pl::sprintf("-%.4x", &[pl::neg(&pos)]) } else { pl::sprintf("%5.4x", &[pos.clone()]) };
        open_(it, &self_, "fgd", f0.clone(), &[0])?;
        open_(it, &self_, "fgd", s(""), &[3])?;
        append_at(&c, 0, &addr.to_bytes());
        if !band(&flag, 0x01).truthy() {
            p = pl::sub(&p, &band(&pos, 0x0f));
        }
        if pl::lt(&p, &pos) {
            open_(it, &self_, "bkg", s(""), &[1, 2])?; // don't underline white space
            cols = pl::sub(&pos, &p);
            let mut nn = pl::mul(&n(3), &cols);
            if pl::gt(&cols, &n(7)) {
                nn = pl::add(&nn, &n(1));
            }
            append_at(&c, 1, rep(" ", &nn).as_bytes());
            append_at(&c, 2, rep(" ", &cols).as_bytes());
            p = pos.clone();
        }
    }
    let bkg_start = my_scalar(it, "bkgStart");
    let bkg_end = my_scalar(it, "bkgEnd");
    // loop through each column of hex numbers
    loop {
        let mut span_class: Vec<Value> = Vec::new();
        let mut span_cont: Vec<Value> = Vec::new();
        let mut span_close = false;
        if pl::ge(&p, &bkg_start) && pl::lt(&p, &bkg_end) {
            for bkg in bkg_span(it).items() {
                if !(pl::ge(&p, &bkg.get("Start")) && pl::lt(&p, &bkg.get("End"))) {
                    continue;
                }
                span_class.push(bkg.get("Class"));
                if pl::num_eq(&pl::add(&p, &n(1)), &bkg.get("End")) {
                    span_close = true;
                } else {
                    span_cont.push(bkg.get("Class")); // this span continues
                }
            }
            let el = if !span_class.is_empty() { pl::concat(&pl::concat(&s("<span class='"), &pl::join(" ", &span_class)), &s("'>")) } else { s("") };
            open_(it, &self_, "bkg", el, &[1, 2])?;
        } else {
            open_(it, &self_, "bkg", s(""), &[1, 2])?;
        }
        open_(it, &self_, "a", name.clone(), &[1, 2])?;
        let mut ch = pl::substr(&block, pl::sub(&pl::sub(&p, &pos), &skipped).to_i64(), Some(1));
        let ord = ch.to_bytes().first().copied().unwrap_or(0);
        append_at(&c, 1, format!("{ord:02x}").as_bytes());
        // make the character HTML-friendly
        pl::tr(&mut ch, "\\x00-\\x1f\\x7f-\\xff", ".", "");
        re("&", "").subst(&mut ch, true, |_| b"&amp;".to_vec());
        re(">", "").subst(&mut ch, true, |_| b"&gt;".to_vec());
        re("<", "").subst(&mut ch, true, |_| b"&lt;".to_vec());
        append_at(&c, 2, &ch.to_bytes());
        p = pl::add(&p, &n(1));
        cols = pl::add(&cols, &n(1));
        // close necessary elements
        if span_close {
            let span_cont_s = if !span_cont.is_empty() { pl::concat(&pl::concat(&s("<span class='"), &pl::join(" ", &span_cont)), &s("'>")) } else { s("") };
            // close without reopening if closing anchor later
            let arg_ = if pl::ge(&pl::sub(&p, &pos), &len) { n(0) } else { span_cont_s };
            open_(it, &self_, "bkg", arg_, &[1, 2])?;
        }
        if dbl_ref && pl::ge(&p, &end_pos) {
            dbl_ref = false;
            id = incr_id(&id);
            let class = if ifd.truthy() { format!("'{} {}'", id.to_text(), ifd_text) } else { id.to_text() };
            let cb = class.into_bytes();
            re(r"class=\w\b", "").subst(&mut name, false, |_| [b"class=".as_slice(), &cb].concat());
            f0 = s("");
            open_(it, &self_, "fgd", f0.clone(), &[0])?;
        }
        if pl::ge(&pl::sub(&p, &pos), &len) {
            open_(it, &self_, "a", s(""), &[1, 2])?; // close our anchor
            break;
        }
        if pl::lt(&cols, &n(16)) {
            append_at(&c, 1, if pl::num_eq(&cols, &n(8)) { b"  " } else { b" " });
            continue;
        } else if band(&flag, 0x01).truthy() && pl::lt(&cols, &len) {
            append_at(&c, 1, b" ");
            continue; // put it all on one line
        }
        if !self_.get("Msg").truthy() {
            append_at(&c, 3, &msg.to_bytes());
            msg = s("");
        }
        for i in 0..4 {
            append_at(&c, i, b"\n"); // add CR to all lines
        }
        self_.set("Msg", n(0));
        // limit data length if specified
        let lim_v = self_.get("Limit");
        if lim_v.truthy() {
            let div = if band(&flag, 0x08).truthy() { n(4) } else { n(1) };
            let lim = pl::sub(&pl::div(&lim_v, &pl::mul(&n(2), &div))?, &n(16));
            let rem = pl::add(&pl::sub(&len, &p), &pos);
            if pl::gt(&pl::sub(&p, &pos), &lim) && pl::gt(&rem, &lim) {
                let nn = ops::bit_and(&pl::sub(&rem, &lim), &ops::bit_not(&n(0x0f)));
                if pl::gt(&nn, &n(16)) {
                    // (no use just cutting out one line)
                    open_(it, &self_, "bkg", s(""), &[1, 2])?; // no underline
                    let mut note = pl::sprintf("[snip %d lines]", &[pl::div(&nn, &n(16))?]);
                    let nl = n(note.to_bytes().len() as i64);
                    note = pl::concat(&s(&rep(" ", &pl::sub(&n(24), &pl::div(&nl, &n(2))?))), &note);
                    append_at(&c, 0, b"  ...\n");
                    let nl = n(note.to_bytes().len() as i64);
                    append_at(&c, 1, &pl::concat(&pl::concat(&note, &s(&rep(" ", &pl::sub(&n(48), &nl)))), &s("\n")).to_bytes());
                    append_at(&c, 2, b"     [snip]     \n");
                    append_at(&c, 3, b"\n");
                    p = pl::add(&p, &nn);
                    skipped = pl::add(&skipped, &pl::sub(&len, &n(blen)));
                }
            }
        }
        let addr = if pl::lt(&p, &n(0)) { pl::sprintf("-%.4x", &[pl::neg(&p)]) } else { pl::sprintf("%5.4x", &[p.clone()]) };
        append_at(&c, 0, &addr.to_bytes());
        cols = n(0);
    }
    if msg.truthy() {
        if self_.get("Msg").truthy() {
            msg = pl::concat(&s(" "), &msg);
        }
        append_at(&c, 3, &msg.to_bytes());
    }
    if band(&flag, 0x100).truthy() && pl::lt(&cols, &n(16)) {
        // continue on same line?
        self_.set("Cont", n(1));
        if msg.truthy() {
            self_.set("Msg", n(1));
        }
    } else {
        for i in 0..4 {
            append_at(&c, i, b"\n");
        }
        self_.set("Msg", n(0));
        self_.set("Cont", n(0));
    }
    // add temporary column data to our real columns
    let real = self_.get("Cols");
    for i in 0..4 {
        append_at(&real, i, &c.at(i as i64).to_bytes());
    }
    self_.delete("TmpCols");
    pl::ret_nothing(want)
}

//------------------------------------------------------------------------------
// Finish dumping of TIFF image data
// Inputs: 0) HtmlDump object ref, 1) ExifTool object ref, 2) length of file
// (this really belongs in Image::ExifTool::Exif, but is placed here so it
//  is only compiled when needed)
pub fn finish_tiff_dump(it: &mut Interp, a: Vec<Value>, want: Want) -> ER<Vec<Value>> {
    let (self_, et, size) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));

    // list of all indirectly referenced TIFF data tags
    let offset_pair: [(&str, &str); 13] = [
        ("StripOffsets", "StripByteCounts"),
        ("TileOffsets", "TileByteCounts"),
        ("FreeOffsets", "FreeByteCounts"),
        ("ThumbnailOffset", "ThumbnailLength"),
        ("PreviewImageStart", "PreviewImageLength"),
        ("JpgFromRawStart", "JpgFromRawLength"),
        ("OtherImageStart", "OtherImageLength"),
        ("PreviewJXLStart", "PreviewJXLLength"),
        ("ImageOffset", "ImageByteCount"),
        ("AlphaOffset", "AlphaByteCount"),
        ("MPImageStart", "MPImageLength"),
        ("IDCPreviewStart", "IDCPreviewLength"),
        ("SamsungRawPointersOffset", "SamsungRawPointersLength"),
    ];

    // add TIFF data to html dump
    'tag: for (tag, pair) in offset_pair {
        let info = pl::method(it, &et, "GetInfo", vec![s(tag)])?;
        if info.is_empty() {
            continue;
        }
        // Panasonic hack: StripOffsets is not valid for Panasonic RW2 files,
        // and StripRowBytes is not valid for some RAW images
        if tag == "StripOffsets" && et.get("TAG_INFO").get(tag).get("PanasonicHack").truthy() {
            // use RawDataOffset instead if available since it is valid in RW2
            let mut info2 = pl::method(it, &et, "GetInfo", vec![s("RawDataOffset")])?;
            if info2.is_empty() {
                info2 = info.clone();
            }
            let keys = info2.keys();
            let offset = keys.first().map(|k| info2.get(&**k)).unwrap_or_default();
            let raf = et.get("RAF");
            // ignore StripByteCounts and assume raw data runs to the end of file
            if keys.len() == 1 && re(r"^\d+$", "").is_match(&offset) && raf.truthy() {
                let pos = pl::method(it, &raf, "Tell", vec![])?;
                pl::method(it, &raf, "Seek", vec![n(0), n(2)])?; // seek to end
                let len = pl::sub(&pl::method(it, &raf, "Tell", vec![])?, &offset);
                pl::method(it, &raf, "Seek", vec![pos, n(0)])?;
                if pl::gt(&len, &n(0)) {
                    let tip = pl::concat(&pl::concat(&s("Size: "), &len), &s(" bytes"));
                    add(it, vec![self_.clone(), offset.clone(), len.clone(), s("(Panasonic raw data)"), tip, n(0x08)], Want::Void)?;
                    continue 'tag;
                }
            }
        }
        // loop through all offsets tags
        for key in info.keys() {
            let key = Value::bytes(&key[..]);
            let _name = pl::call(it, "Image::ExifTool::GetTagName", vec![key.clone()])?;
            let grp1 = pl::method(it, &et, "GetGroup", vec![key.clone(), n(1)])?;
            let opts = Value::new_hash();
            opts.set("Group1", grp1.clone());
            let info2 = pl::method(it, &et, "GetInfo", vec![s(pair), opts])?;
            let mut key2 = s(pair);
            if let Some(c) = re(r"( .*)", "").m(&key) {
                key2 = pl::concat(&key2, &cap(&c, 1)); // use same instance number as $tag
            }
            if !hget(&info2, &key2).truthy() {
                continue;
            }
            let offsets = hget(&info, &key);
            let byte_counts = hget(&info2, &key2);
            // ignore primary MPImage (this is the whole JPEG)
            if tag == "MPImageStart" && pl::str_eq(&offsets, "0") {
                continue;
            }
            // (long lists may be SCALAR references)
            let mut offsets_l = pl::split_ws(&if offsets.is_ref() { offsets.deref() } else { offsets.clone() });
            let mut byte_counts_l = pl::split_ws(&if byte_counts.is_ref() { byte_counts.deref() } else { byte_counts.clone() });
            let num = offsets_l.len();
            let mut li = 0usize;
            let mut pad_bytes = 0i64;
            let mut i = 0usize;
            while !offsets_l.is_empty() && !byte_counts_l.is_empty() {
                let offset = offsets_l.remove(0);
                let byte_count = byte_counts_l.remove(0);
                let mut end = pl::add(&offset, &byte_count);
                if !offsets_l.is_empty() && !byte_counts_l.is_empty() {
                    // show data as contiguous if only normal pad bytes between blocks
                    if band(&end, 0x01).truthy() && pl::num_eq(&pl::add(&end, &n(1)), &offsets_l[0]) {
                        end = pl::add(&end, &n(1));
                        pad_bytes += 1; // count them
                    }
                    if pl::num_eq(&end, &offsets_l[0]) {
                        // combine these two blocks
                        byte_counts_l[0] = pl::add(&byte_counts_l[0], &pl::sub(&offsets_l[0], &offset));
                        offsets_l[0] = offset;
                        i += 1;
                        continue;
                    }
                }
                let mut msg = pl::concat(&pl::concat(&pl::method(it, &et, "GetGroup", vec![key.clone(), n(1)])?, &s(":")), &s(tag));
                re(r"(Offsets?|Start)$", "").subst(&mut msg, false, |_| b" ".to_vec());
                if num > 1 {
                    if li != i {
                        msg = pl::concat(&msg, &s(&format!("{li}-")));
                    }
                    msg = pl::concat(&msg, &s(&format!("{i} ")));
                    li = i + 1;
                }
                msg = pl::concat(&msg, &s("data"));
                let mut tip = pl::concat(&pl::concat(&s("Size: "), &byte_count), &s(" bytes"));
                if pad_bytes != 0 {
                    tip = pl::concat(&tip, &s(&format!(", incl. {pad_bytes} pad bytes")));
                }
                let m = pl::concat(&pl::concat(&s("("), &msg), &s(")"));
                add(it, vec![self_.clone(), offset.clone(), byte_count.clone(), m, tip, n(0x08)], Want::Void)?;
                i += 1;
            }
        }
    }
    // find offset of last dumped information, and dump any unknown trailer
    let mut last = n(0);
    let block = self_.get("Block");
    for start in block.keys() {
        for block_info in block.get(&*start).items() {
            let end = pl::add(&Value::bytes(&start[..]), &block_info.at(0));
            if pl::lt(&last, &end) {
                last = end;
            }
        }
    }
    let diff = pl::sub(&size, &last);
    if pl::gt(&diff, &n(0)) && (last.truthy() || pl::method(it, &et, "Options", vec![s("Unknown")])?.truthy()) {
        if pl::gt(&diff, &n(1)) || band(&size, 0x01).truthy() {
            let tip = pl::concat(&pl::concat(&s("Size: "), &diff), &s(" bytes"));
            add(it, vec![self_.clone(), last.clone(), diff.clone(), s("[unknown data]"), tip, n(0x08)], Want::Void)?;
        } else {
            add(it, vec![self_.clone(), last.clone(), diff.clone(), s("[trailing pad byte]"), Value::Undef, n(0x08)], Want::Void)?;
        }
    }
    pl::ret_nothing(want)
}

//------------------------------------------------------------------------------
// utility routine to write to file or memory
// Inputs: 0) file or scalar reference, 1-N) list of stuff to write
// Returns: true on success
pub fn write(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let outfile = arg(&a, 0);
    let items = &a[1.min(a.len())..];
    if pl::call(it, "UNIVERSAL::isa", vec![outfile.clone(), s("GLOB")])?.truthy() {
        let r = pl::print_to(it, Some(&outfile), items)?;
        return ret(if r { n(1) } else { s("") });
    } else if pl::ref_of(&outfile) == "SCALAR" {
        let cur = outfile.deref();
        let mut b = cur.to_bytes().to_vec();
        for v in items {
            b.extend_from_slice(&v.to_bytes());
        }
        pl::set_deref(&outfile, Value::bytes(b))?;
        return ret(n(1));
    }
    ret(n(0))
}

pub fn register(it: &mut Interp) {
    it.define_native("Image::ExifTool::HtmlDump::new", new);
    it.define_native("Image::ExifTool::HtmlDump::Add", add);
    it.define_native("Image::ExifTool::HtmlDump::Print", print);
    it.define_native("Image::ExifTool::HtmlDump::Open", open);
    it.define_native("Image::ExifTool::HtmlDump::DumpTable", dump_table);
    it.define_native("Image::ExifTool::HtmlDump::FinishTiffDump", finish_tiff_dump);
    it.define_native("Image::ExifTool::HtmlDump::Write", write);
}
