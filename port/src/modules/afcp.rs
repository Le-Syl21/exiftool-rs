//! Image::ExifTool::AFCP (Image/ExifTool/AFCP.pm), ported sub by sub (see docs/port/PORTING.md).
//!
//! Read/write the AFCP trailer.

use crate::interp::eval::{Cell, ER};
use crate::interp::{Interp, Want};
use crate::modules::random_access::{cell, raf_read, raf_read_cell};
use crate::perl::Value;
use crate::pl::{self, arg, cap, n, ret};
use crate::runtime;

fn s(x: &str) -> Value {
    pl::s(x)
}

fn call(it: &mut Interp, name: &str, args: Vec<Value>) -> ER<Value> {
    pl::call(it, &format!("Image::ExifTool::{name}"), args)
}

//------------------------------------------------------------------------------
// Read/write AFCP information in a file
// Inputs: 0) ExifTool object reference, 1) dirInfo reference
// (Set 'ScanForTrailer' member in dirInfo to scan from current position for AFCP)
// Returns: 1 on success, 0 if this file didn't contain AFCP information
//          -1 on write error or if the offsets were incorrect on reading
// - updates DataPos to point to actual AFCP start if ScanForTrailer is set
// - updates DirLen to trailer length
// - returns Fixup reference in dirInfo hash when writing
pub fn process_afcp(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info) = (arg(&a, 0), arg(&a, 1));
    let raf = dir_info.get("RAF");
    pl::at(76);
    let cur_pos = pl::method(it, &raf, "Tell", vec![])?;
    let o = dir_info.get("Offset");
    let offset = if o.truthy() { o } else { n(0) }; // offset from end of file
    let mut rtn_val = n(0);
    let bget = |c: &Cell| c.borrow().clone();

    'no_afcp: {
        let buff = cell(Value::Undef); // (my $buff: \$buff is taken below)
        let buff_pt = Value::ScalarRef(buff.clone());
        let fix;
        let (mut dir_buff, mut val_buff, mut fixup, mut vers) = (Value::Undef, Value::Undef, Value::Undef, Value::Undef);
        // look for AXS trailer
        if !pl::method(it, &raf, "Seek", vec![pl::sub(&n(-12), &offset), n(2)])?.truthy() || !pl::num_eq(&raf_read_cell(it, &raf, &buff, n(12))?, &n(12)) {
            break 'no_afcp;
        }
        let Some(m) = pl::re(r"^(AXS(!|\*))", "").m(&bget(&buff)) else { break 'no_afcp };
        let end_pos = pl::method(it, &raf, "Tell", vec![])?;
        let hdr = cap(&m, 1);
        call(it, "SetByteOrder", vec![s(if pl::str_eq(&cap(&m, 2), "!") { "MM" } else { "II" })])?;
        let start_pos = call(it, "Get32u", vec![buff_pt.clone(), n(4)])?;
        // (/^$hdr/ and /$hdr/: the header is interpolated as a pattern)
        let hdr_b = hdr.to_bytes();
        let hdr_start = pl::re_dyn(&[b"^".as_slice(), &hdr_b].concat(), "")?;
        let hdr_any = crate::perl::regex::compile(&hdr_b, "").map_err(|e| crate::interp::Flow::Die(Value::from(format!("{e}\n"))))?;
        if pl::method(it, &raf, "Seek", vec![start_pos.clone(), n(0)])?.truthy() && pl::num_eq(&raf_read_cell(it, &raf, &buff, n(12))?, &n(12)) && hdr_start.is_match(&bget(&buff)) {
            fix = n(0);
        } else {
            rtn_val = n(-1);
            // look for start of AXS trailer if 'ScanForTrailer'
            if !(dir_info.get("ScanForTrailer").truthy() && pl::method(it, &raf, "Seek", vec![cur_pos.clone(), n(0)])?.truthy()) {
                break 'no_afcp;
            }
            let mut actual_pos = cur_pos.clone();
            // first look for header right at current position
            loop {
                if pl::num_eq(&raf_read_cell(it, &raf, &buff, n(12))?, &n(12)) && hdr_start.is_match(&bget(&buff)) {
                    break;
                }
                if !pl::num_eq(&actual_pos, &cur_pos) {
                    break 'no_afcp;
                }
                // scan for AXS header (could be after preview image)
                loop {
                    let mut buf2 = Value::Undef;
                    if !raf_read(it, &raf, &mut buf2, n(65536))?.truthy() {
                        break 'no_afcp;
                    }
                    let b = pl::concat(&bget(&buff), &buf2);
                    *buff.borrow_mut() = b.clone();
                    // ($buff =~ /$hdr/g: the assignment above reset pos($buff))
                    if let Some(m) = hdr_any.match_at(&b.to_bytes(), 0).ok().flatten() {
                        // pos($buff) is the end of the match
                        let end = m[0].map_or(0, |(_, e)| e);
                        actual_pos = pl::sub(&pl::add(&actual_pos, &n(end as i64)), &pl::length(&hdr));
                        break; // ok, now go back and re-read header
                    }
                    buf2 = pl::substr(&buf2, -3, None); // only need last 3 bytes for next test
                    actual_pos = pl::add(&actual_pos, &pl::sub(&pl::length(&b), &pl::length(&buf2)));
                    *buff.borrow_mut() = buf2;
                }
                if !pl::method(it, &raf, "Seek", vec![actual_pos.clone(), n(0)])?.truthy() {
                    break; // seek to start of AFCP
                }
            }
            // calculate shift for fixing AFCP offsets
            fix = pl::sub(&actual_pos, &start_pos);
        }
        // set variables returned in dirInfo hash
        dir_info.set("DataPos", pl::add(&start_pos, &fix)); // actual start position
        dir_info.set("DirLen", pl::sub(&end_pos, &pl::add(&start_pos, &fix)));

        rtn_val = n(1);
        let verbose = pl::method(it, &et, "Options", vec![s("Verbose")])?;
        let out = pl::method(it, &et, "Options", vec![s("TextOut")])?;
        let outfile = dir_info.get("OutFile");
        if outfile.truthy() {
            // allow all AFCP information to be deleted
            if et.get("DEL_GROUP").get("AFCP").truthy() {
                if verbose.truthy() {
                    pl::print_to(it, Some(&out), &[s("  Deleting AFCP\n")])?;
                }
                et.set("CHANGED", pl::add(&et.get("CHANGED"), &n(1)));
                break 'no_afcp;
            }
            dir_buff = s("");
            val_buff = s("");
            runtime::require(it, "Image::ExifTool::Fixup")?;
            fixup = dir_info.get("Fixup");
            if !fixup.truthy() {
                fixup = pl::method(it, &s("Image::ExifTool::Fixup"), "new", vec![])?;
                dir_info.set("Fixup", fixup.clone());
            }
            vers = pl::substr(&bget(&buff), 4, Some(2)); // get version number
        } else if verbose.truthy() || et.get("HTML_DUMP").truthy() {
            pl::method(it, &et, "DumpTrailer", vec![dir_info.clone()])?;
        }
        // read AFCP directory data
        let num_entries = call(it, "Get16u", vec![buff_pt.clone(), n(6)])?;
        let mut dir = Value::Undef;
        let dlen = pl::mul(&n(12), &num_entries);
        if !pl::num_eq(&raf_read(it, &raf, &mut dir, dlen.clone())?, &dlen) {
            pl::method(it, &et, "Error", vec![s("Error reading AFCP directory"), n(1)])?;
            break 'no_afcp;
        }
        if pl::gt(&verbose, &n(2)) && !outfile.truthy() {
            let dat = pl::concat(&bget(&buff), &dir);
            pl::print_to(it, Some(&out), &[s("  AFCP Directory:\n")])?;
            pl::method(it, &et, "VerboseDump", vec![pl::sref(dat), s("Addr"), dir_info.get("DataPos"), s("Width"), n(12)])?;
        }
        if fix.truthy() {
            let msg = pl::concat(&s("Adjusted AFCP offsets by "), &fix);
            pl::method(it, &et, "Warn", vec![msg, n(1)])?;
        }
        //
        // process AFCP directory
        //
        let tag_table_ptr = call(it, "GetTagTable", vec![s("Image::ExifTool::AFCP::Main")])?;
        let dir_pt = pl::sref(dir.clone());
        let mut index: i64 = 0;
        while pl::lt(&n(index), &num_entries) {
            let entry = 12 * index;
            let tag = pl::substr(&dir, entry, Some(4));
            let mut size = call(it, "Get32u", vec![dir_pt.clone(), n(entry + 4)])?;
            let offset = call(it, "Get32u", vec![dir_pt.clone(), n(entry + 8)])?;
            if pl::lt(&size, &n(0x80000000))
                && pl::method(it, &raf, "Seek", vec![pl::add(&offset, &fix), n(0)])?.truthy()
                && pl::num_eq(&raf_read_cell(it, &raf, &buff, size.clone())?, &size)
            {
                if outfile.truthy() {
                    // rewrite this information
                    let tag_info = pl::method(it, &et, "GetTagInfo", vec![tag_table_ptr.clone(), tag.clone()])?;
                    if tag_info.truthy() && tag_info.get("SubDirectory").truthy() {
                        let subdir_info = Value::new_hash();
                        subdir_info.set("DataPt", buff_pt.clone());
                        subdir_info.set("DirStart", n(0));
                        subdir_info.set("DirLen", size.clone());
                        subdir_info.set("DataPos", pl::add(&offset, &fix));
                        subdir_info.set("Parent", s("AFCP"));
                        let sub_table = call(it, "GetTagTable", vec![tag_info.path(&["SubDirectory", "TagTable"])])?;
                        let new_dir = pl::method(it, &et, "WriteDirectory", vec![subdir_info, sub_table])?;
                        if !new_dir.is_undef() {
                            size = pl::length(&new_dir);
                            *buff.borrow_mut() = new_dir;
                        }
                    }
                    pl::method(it, &fixup, "AddFixup", vec![pl::add(&pl::length(&dir_buff), &n(8))])?;
                    let p1 = call(it, "Set32u", vec![size.clone()])?;
                    let p2 = call(it, "Set32u", vec![pl::length(&val_buff)])?;
                    dir_buff = pl::concat(&dir_buff, &pl::concat(&pl::concat(&tag, &p1), &p2));
                    val_buff = pl::concat(&val_buff, &bget(&buff));
                } else {
                    // extract information
                    pl::method(
                        it,
                        &et,
                        "HandleTag",
                        vec![tag_table_ptr.clone(), tag.clone(), bget(&buff), s("DataPt"), buff_pt.clone(), s("Size"), size.clone(), s("Index"), n(index), s("DataPos"), pl::add(&offset, &fix)],
                    )?;
                }
            } else {
                pl::method(it, &et, "Warn", vec![s("Bad AFCP directory")])?;
                if outfile.truthy() {
                    rtn_val = n(-1);
                }
                break;
            }
            index += 1;
        }
        if outfile.truthy() && pl::length(&dir_buff).truthy() {
            let out_pos = call(it, "Tell", vec![outfile.clone()])?; // get current outfile position
            // apply fixup to directory pointers
            let val_pos = pl::add(&out_pos, &n(12)); // start of value data
            fixup.set("Shift", pl::add(&fixup.get("Shift"), &pl::add(&val_pos, &pl::length(&dir_buff))));
            let db = pl::sref(dir_buff.clone());
            pl::method(it, &fixup, "ApplyFixup", vec![db.clone()])?;
            dir_buff = db.deref();
            // write the AFCP header, directory, value data and EOF record (with zero checksums)
            let nd = call(it, "Set16u", vec![pl::div(&pl::length(&dir_buff), &n(12))?])?;
            let z = call(it, "Set32u", vec![n(0)])?;
            let op = call(it, "Set32u", vec![out_pos.clone()])?;
            let args = vec![outfile.clone(), hdr.clone(), vers.clone(), nd, z.clone(), dir_buff.clone(), val_buff.clone(), hdr.clone(), op, z];
            if !call(it, "Write", args)?.truthy() {
                rtn_val = n(-1);
            }
            // complete fixup so the calling routine can apply further shifts
            pl::method(it, &fixup, "AddFixup", vec![pl::add(&pl::add(&pl::length(&dir_buff), &pl::length(&val_buff)), &n(4))])?;
            fixup.set("Start", pl::add(&fixup.get("Start"), &val_pos));
            fixup.set("Shift", pl::sub(&fixup.get("Shift"), &val_pos));
        }
    }
    ret(rtn_val)
}

pub fn register(it: &mut Interp) {
    it.define_native("Image::ExifTool::AFCP::ProcessAFCP", process_afcp);
}
