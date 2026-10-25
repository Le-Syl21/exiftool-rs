//! Image::ExifTool::PCAP (Image/ExifTool/PCAP.pm), ported sub by sub (see docs/port/PORTING.md).

use crate::interp::eval::{Cell, ER};
use crate::interp::{Interp, Want};
use crate::modules::exiftool::part2::{get16u, get32u};
use crate::modules::random_access::{cell, raf_read_cell};
use crate::perl::Value;
use crate::pl::{self, arg, cap, n, re, ret};

fn val(c: &Cell) -> Value {
    c.borrow().clone()
}

/// `\$var` of a `my` variable kept in a cell.
fn rf(c: &Cell) -> Value {
    Value::ScalarRef(c.clone())
}

/// `$raf->Read($buff, $len) == $len`
fn read_eq(it: &mut Interp, raf: &Value, buff: &Cell, len: &Value) -> ER<bool> {
    let r = raf_read_cell(it, raf, buff, len.clone())?;
    Ok(pl::num_eq(&r, len))
}

fn seek(it: &mut Interp, raf: &Value, pos: Value, whence: i64) -> ER<bool> {
    Ok(pl::method(it, raf, "Seek", vec![pos, n(whence)])?.truthy())
}

fn g16u(it: &mut Interp, data_pt: &Value, pos: Value) -> ER<Value> {
    Ok(get16u(it, vec![data_pt.clone(), pos], Want::Scalar)?.pop().unwrap_or_default())
}

fn g32u(it: &mut Interp, data_pt: &Value, pos: Value) -> ER<Value> {
    Ok(get32u(it, vec![data_pt.clone(), pos], Want::Scalar)?.pop().unwrap_or_default())
}

fn handle_tag(it: &mut Interp, et: &Value, table: &Value, tag: Value, v: Value) -> ER<()> {
    pl::method(it, et, "HandleTag", vec![table.clone(), tag, v])?;
    Ok(())
}

/// Extract metadata from a PCAP file
/// Inputs: 0) ExifTool ref, 1) dirInfo ref
/// Returns: 1 on success, 0 if this wasn't a valid PCAP file
pub fn process_pcap(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info) = (arg(&a, 0), arg(&a, 1));
    let raf = dir_info.get("RAF");
    let buff = cell(Value::Undef);
    let (tag_table_ptr, ty, ts, byte_order, ver_pos);

    // verify this is a valid PCAP file
    if !read_eq(it, &raf, &buff, &n(24))? {
        return ret(n(0));
    }

    if let Some(m) = re(r"^\xa1\xb2(\xc3\xd4|\x3c\x4d)\0.\0.|(\xd4\xc3|\x4d\x3c)\xb2\xa1.\0.\0", "s").m(&val(&buff)) {
        ty = pl::s("PCAP");
        let tmp = if cap(&m, 1).truthy() { cap(&m, 1) } else { cap(&m, 2) };
        let b = tmp.to_bytes();
        et.set("TSResol", Value::Num(if &b[..] == b"\xc3\xd4" || &b[..] == b"\xd4\xc3" { 1e-6 } else { 1e-9 }));
        byte_order = pl::s(if re(r"^\xa1", "").is_match(&val(&buff)) { "MM" } else { "II" });
        ver_pos = 4;
    } else if let Some(m) = re(r"^\x0a\x0d\x0d\x0a.{4}(\x1a\x2b\x3c\x4d|\x4d\x3c\x2b\x1a)", "s").m(&val(&buff)) {
        ty = pl::s("PCAPNG");
        byte_order = pl::s(if &cap(&m, 1).to_bytes()[..] == b"\x1a\x2b\x3c\x4d" { "MM" } else { "II" });
        ver_pos = 12;
    } else if re(r"^GMBU\0\x02", "").is_match(&val(&buff)) {
        // handle Windows Network Monitor 2.0 CAP files
        // http://web.archive.org/web/20240430011527/https://learn.microsoft.com/en-us/windows/win32/netmon2/capturefile-header-values
        let static_vars = pl::pkg_hash(it, "Image::ExifTool::static_vars");
        static_vars.get_or_new_hash("OverrideFileDescription").set("CAP", pl::s("Microsoft Network Monitor Capture"));
        pl::method(it, &et, "SetFileType", vec![pl::s("CAP"), pl::s("application/octet-stream"), pl::s("cap")])?;
        let tag_table_ptr = pl::call(it, "Image::ExifTool::GetTagTable", vec![pl::s("Image::ExifTool::PCAP::Main")])?;
        let a = pl::unpack("x6v*", &val(&buff))?;
        et.set("WindowsTS", n(1));
        let slice: Vec<Value> = [1usize, 2, 4, 5, 6, 7, 8].iter().map(|&i| arg(&a, i)).collect();
        let v = pl::sprintf("%.4d:%.2d:%.2d %.2d:%.2d:%.2d.%.3d", &slice);
        handle_tag(it, &et, &tag_table_ptr, pl::s("LinkType"), arg(&a, 0))?;
        handle_tag(it, &et, &tag_table_ptr, pl::s("TimeStamp"), v)?;
        if seek(it, &raf, n(40), 0)? && read_eq(it, &raf, &buff, &n(8))? {
            let u = pl::unpack("V2", &val(&buff))?;
            let (off, len) = (arg(&u, 0), arg(&u, 1));
            // extract comment
            if pl::lt(&len, &n(1024)) && seek(it, &raf, off, 0)? && read_eq(it, &raf, &buff, &len)? {
                handle_tag(it, &et, &tag_table_ptr, n(1), val(&buff))?; // (NC) null terminated? Unicode?
            }
        }
        return ret(n(1));
    } else {
        return ret(n(0));
    }
    pl::method(it, &et, "SetFileType", vec![ty.clone()])?;
    pl::call(it, "Image::ExifTool::SetByteOrder", vec![byte_order.clone()])?;
    tag_table_ptr = pl::call(it, "Image::ExifTool::GetTagTable", vec![pl::s("Image::ExifTool::PCAP::Main")])?;
    let ver = pl::concat(&pl::concat(&g16u(it, &rf(&buff), n(ver_pos))?, &pl::s(".")), &g16u(it, &rf(&buff), n(ver_pos + 2))?);
    handle_tag(it, &et, &tag_table_ptr, pl::s("PCAPVersion"), pl::concat(&pl::concat(&ty, &pl::s(" ")), &ver))?;
    handle_tag(it, &et, &tag_table_ptr, pl::s("ByteOrder"), byte_order.clone())?;

    if pl::str_eq(&ty, "PCAP") {
        let lt = g16u(it, &rf(&buff), n(22))?;
        handle_tag(it, &et, &tag_table_ptr, pl::s("LinkType"), lt)?;
        if !read_eq(it, &raf, &buff, &n(8))? {
            pl::method(it, &et, "Warn", vec![pl::s("Truncated PCAP file")])?;
            return ret(n(1));
        }
        ts = pl::add(&g32u(it, &rf(&buff), n(0))?, &pl::mul(&g32u(it, &rf(&buff), n(4))?, &et.get("TSResol")));
        handle_tag(it, &et, &tag_table_ptr, pl::s("TimeStamp"), ts)?;
        return ret(n(1));
    }
    // read through PCAPNG options for the SHB, IDB and get the timestamp from the first EPD
    let mut dir = "SHB"; // Section Header Block
    loop {
        if !read_eq(it, &raf, &buff, &n(4))? {
            break;
        }
        let opt = g16u(it, &rf(&buff), n(0))?;
        let len = g16u(it, &rf(&buff), n(2))?;
        if pl::num_eq(&opt, &n(0)) {
            // (end of options?)
            if !read_eq(it, &raf, &buff, &n(20))? {
                break;
            }
            let dir_num = g32u(it, &rf(&buff), n(4))?;
            if pl::num_eq(&dir_num, &n(1)) {
                let lt = g16u(it, &rf(&buff), n(12))?;
                handle_tag(it, &et, &tag_table_ptr, pl::s("LinkType"), lt)?;
                dir = "IDB"; // Interface Description Block
                continue; // continue with IDB options
            } else if pl::num_eq(&dir_num, &n(6)) {
                // EPD (Enhanced Packet Data)
                let mut ts = pl::mul(&Value::Int(4294967296), &g32u(it, &rf(&buff), n(16))?);
                // ($raf->Read(\$buff, 4): the data goes to the temporary reference, not to $buff)
                let mut args = [rf(&buff), n(4)];
                let r = pl::method_mut(it, &raf, "Read", &mut args, Want::Scalar)?.pop().unwrap_or_default();
                if !pl::num_eq(&r, &n(4)) {
                    break;
                }
                if et.get("TSResol").truthy() {
                    let off = {
                        let o = et.get("TSOff");
                        if o.truthy() {
                            o
                        } else {
                            n(0)
                        }
                    };
                    ts = pl::add(&pl::mul(&pl::add(&ts, &g32u(it, &rf(&buff), n(0))?), &et.get("TSResol")), &off);
                    handle_tag(it, &et, &tag_table_ptr, pl::s("TimeStamp"), ts)?;
                }
            }
            break;
        }
        let nn = crate::perl::ops::bit_and(&pl::add(&len, &n(3)), &n(0xfffc)); // round to an even 4 bytes
        if !read_eq(it, &raf, &buff, &nn)? {
            pl::method(it, &et, "Warn", vec![pl::s(&format!("Error reading {dir} options"))])?;
            break;
        }
        let id = if tag_table_ptr.get(opt.to_bytes()).truthy() { opt.clone() } else { pl::concat(&pl::s(&format!("{dir}-")), &opt) };
        let tell = pl::method(it, &raf, "Tell", vec![])?;
        pl::method(
            it,
            &et,
            "HandleTag",
            vec![tag_table_ptr.clone(), id, Value::Undef, pl::s("DataPt"), rf(&buff), pl::s("DataPos"), pl::sub(&tell, &nn), pl::s("Size"), len, pl::s("MakeTagInfo"), n(1)],
        )?;
    }
    ret(n(1))
}

/// Register the subs ported so far; the others still run from the Perl.
pub fn register(it: &mut Interp) {
    it.define_native("Image::ExifTool::PCAP::ProcessPCAP", process_pcap);
}
