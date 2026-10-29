//! Image::ExifTool::Ogg (Image/ExifTool/Ogg.pm), ported sub by sub (see docs/port/PORTING.md).
//!
//! Read Ogg meta information

use crate::interp::eval::ER;
use crate::interp::{Interp, Want};
use crate::modules::random_access::{cell, raf_read_cell, substr};
use crate::perl::Value;
use crate::pl::{self, arg, cap, n, re, ret};

const MAX_PACKETS: i64 = 2; // maximum packets to scan from each stream at start of file

/// `Image::ExifTool::NAME(ARGS)` (the functions imported with :DataAccess :Utils).
fn call(it: &mut Interp, name: &str, args: Vec<Value>) -> ER<Value> {
    pl::call(it, &format!("Image::ExifTool::{name}"), args)
}

/// `ord($ch)`
fn ord(v: &Value) -> Value {
    n(v.to_bytes().first().map_or(0, |b| *b as i64))
}

fn process_packet_impl(it: &mut Interp, et: &Value, data_pt: &Value) -> ER<Value> {
    let mut rtn_val = n(0);
    pl::at(46);
    let data = pl::sderef(data_pt)?;
    let m = re(r"^(.)(vorbis|theora)", "s").m(&data).or_else(|| re(r"^(OpusHead|OpusTags)", "").m(&data));
    if let Some(m) = m {
        let (tag, type_, pos) = if cap(&m, 2).truthy() { (ord(&cap(&m, 1)), pl::ucfirst(&cap(&m, 2)), n(7)) } else { (cap(&m, 1), pl::s("Opus"), n(8)) };
        // this is an OGV file if it contains Theora video
        if type_.eq_str("Theora") && et.get("FILE_TYPE").eq_str("OGG") {
            pl::method(it, et, "OverrideFileType", vec![pl::s("OGV")])?;
        }
        if type_.eq_str("Opus") && et.get("FILE_TYPE").eq_str("OGG") {
            pl::method(it, et, "OverrideFileType", vec![pl::s("OPUS")])?;
        }
        let tag_table_ptr = call(it, "GetTagTable", vec![Value::from(format!("Image::ExifTool::{}::Main", type_.to_text()))])?;
        let tag_info = pl::method(it, et, "GetTagInfo", vec![tag_table_ptr, tag])?;
        if !(tag_info.truthy() && tag_info.get("SubDirectory").truthy()) {
            return Ok(n(0));
        }
        let subdir = tag_info.get("SubDirectory");
        let dir_info = Value::new_hash();
        dir_info.set("DataPt", data_pt.clone());
        dir_info.set("DirName", tag_info.get("Name"));
        dir_info.set("DirStart", pos);
        let table = call(it, "GetTagTable", vec![subdir.get("TagTable")])?;
        // set group1 so Theoris comments can be distinguised from Vorbis comments
        if type_.eq_str("Theora") {
            et.set("SET_GROUP1", type_.clone());
        }
        if subdir.get("ByteOrder").truthy() {
            call(it, "SetByteOrder", vec![subdir.get("ByteOrder")])?;
        }
        rtn_val = pl::method(it, et, "ProcessDirectory", vec![dir_info, table])?;
        call(it, "SetByteOrder", vec![pl::s("II")])?;
        et.delete("SET_GROUP1");
    }
    Ok(rtn_val)
}

/// Process Ogg packet
/// Inputs: 0) ExifTool object ref, 1) data ref
/// Returns: 1 on success
pub fn process_packet(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    ret(process_packet_impl(it, &arg(&a, 0), &arg(&a, 1))?)
}

/// Extract information from an Ogg file
/// Inputs: 0) ExifTool object reference, 1) dirInfo reference
/// Returns: 1 on success, 0 if this wasn't a valid Ogg file
pub fn process_ogg(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info) = (arg(&a, 0), arg(&a, 1));

    // must first check for leading/trailing ID3 information
    if !et.get("DoneID3").truthy() {
        crate::runtime::require(it, "Image::ExifTool::ID3")?;
        if pl::call(it, "Image::ExifTool::ID3::ProcessID3", vec![et.clone(), dir_info.clone()])?.truthy() {
            return ret(n(1));
        }
    }
    let mut raf = dir_info.get("RAF");
    let verbose = pl::method(it, &et, "Options", vec![pl::s("Verbose")])?;
    let out = pl::method(it, &et, "Options", vec![pl::s("TextOut")])?;
    let (mut success, mut page, mut packets, mut streams, mut stream) = (n(0), n(0), n(0), n(0), pl::s(""));
    let buff = cell(Value::Undef);
    let buff_ref = Value::ScalarRef(buff.clone());
    let mut flag;
    let mut num_flac = Value::Undef;
    let val = Value::new_hash();
    let stream_page = Value::new_hash();
    let b = || buff.borrow().clone();

    loop {
        // must read ahead to next page to see if it is a continuation
        // (this code would be a lot simpler if the continuation flag
        //  was on the leading instead of the trailing page!)
        if raf.truthy() && pl::num_eq(&raf_read_cell(it, &raf, &buff, n(28))?, &n(28)) {
            // validate magic number
            if !re(r"^OggS", "").is_match(&b()) {
                if success.truthy() {
                    pl::method(it, &et, "Warn", vec![pl::s("Lost synchronization")])?;
                }
                break;
            }
            if !success.truthy() {
                // set file type and initialize on first page
                success = n(1);
                pl::method(it, &et, "SetFileType", vec![])?;
                call(it, "SetByteOrder", vec![pl::s("II")])?;
            }
            flag = call(it, "Get8u", vec![buff_ref.clone(), n(5)])?; // page flag
            stream = call(it, "Get32u", vec![buff_ref.clone(), n(14)])?; // stream serial number
            if flag.to_i64() & 0x02 != 0 {
                streams = pl::add(&streams, &n(1)); // count start-of-stream pages
                page = n(0);
                stream_page.set(stream.to_bytes(), n(0));
            } else {
                page = stream_page.get(stream.to_bytes());
            }
            if flag.to_i64() & 0x01 == 0 {
                packets = pl::add(&packets, &n(1)); // keep track of packet count
            }
        } else {
            // all done unless we have to process our last packet
            if val.is_empty() {
                break;
            }
            let mut keys: Vec<Value> = val.keys().into_iter().map(Value::from).collect();
            keys.sort_by(|x, y| x.to_bytes().cmp(&y.to_bytes()));
            stream = keys[0].clone(); // take a stream
            flag = n(0); // no continuation
            raf = Value::Undef; // flag for done reading
        }

        if !num_flac.is_undef() {
            // stop to process FLAC headers if we hit the end of file
            if !raf.truthy() {
                break;
            }
            num_flac = pl::sub(&num_flac, &n(1)); // one less header packet to read
        } else {
            // can finally process previous packet from this stream
            // unless this is a continuation page
            if !val.get(stream.to_bytes()).is_undef() && flag.to_i64() & 0x01 == 0 {
                process_packet_impl(it, &et, &pl::elem_ref(&val, &stream.to_bytes()))?;
                val.delete(stream.to_bytes());
                // only read the first $MAX_PACKETS packets from each stream
                if (pl::gt(&packets, &pl::mul(&n(MAX_PACKETS), &streams)) || raf.is_undef()) && val.is_empty() {
                    break; // all done (success!)
                }
            }
            // stop processing Ogg if we have scanned enough packets
            if pl::gt(&packets, &pl::mul(&n(MAX_PACKETS), &streams)) && val.is_empty() {
                break;
            }
        }

        // continue processing the current page
        let page_num = call(it, "Get32u", vec![buff_ref.clone(), n(18)])?; // page sequence number
        let nseg = call(it, "Get8u", vec![buff_ref.clone(), n(26)])?; // number of segments
        // calculate total data length
        let mut data_len = call(it, "Get8u", vec![buff_ref.clone(), n(27)])?;
        if nseg.truthy() {
            if !raf.truthy() {
                break;
            }
            let len = pl::sub(&nseg, &n(1));
            if !pl::num_eq(&raf_read_cell(it, &raf, &buff, len.clone())?, &len) {
                break;
            }
            let segs = pl::unpack("C*", &b())?;
            // could check that all these (but the last) are 255...
            for s in &segs {
                data_len = pl::add(&data_len, s);
            }
        }
        if !page.is_undef() {
            if pl::num_eq(&page, &page_num) {
                page = pl::add(&page, &n(1));
                stream_page.set(stream.to_bytes(), page.clone());
            } else {
                pl::method(it, &et, "Warn", vec![pl::s("Missing page(s) in Ogg file")])?;
                page = Value::Undef;
                stream_page.delete(stream.to_bytes());
            }
        }
        // read page data
        if !(raf.truthy() && pl::num_eq(&raf_read_cell(it, &raf, &buff, data_len.clone())?, &data_len)) {
            break;
        }
        if pl::gt(&verbose, &n(1)) {
            let msg = pl::sprintf("Page %d, stream 0x%x, flag 0x%x (%d bytes)\n", &[page_num.clone(), stream.clone(), flag.clone(), data_len.clone()]);
            pl::print_to(it, Some(&out), &[msg])?;
            let tell = pl::method(it, &raf, "Tell", vec![])?;
            pl::method(it, &et, "VerboseDump", vec![buff_ref.clone(), pl::s("DataPos"), pl::sub(&tell, &data_len)])?;
        }
        let cur = val.get(stream.to_bytes());
        if !cur.is_undef() {
            val.set(stream.to_bytes(), pl::concat(&cur, &b())); // add this continuation page
        } else if flag.to_i64() & 0x01 == 0 {
            // ignore remaining pages of a continued packet
            // ignore the first page of any packet we aren't parsing
            let bb = b();
            if re(r"^(.(vorbis|theora)|Opus(Head|Tags))", "s").is_match(&bb) {
                val.set(stream.to_bytes(), bb); // save this page
            } else if let Some(m) = re(r"^\x7fFLAC..(..)", "s").m(&bb) {
                num_flac = pl::unpack1("n", &cap(&m, 1))?;
                let s = substr(it, &bb, &n(9), None)?;
                val.set(stream.to_bytes(), s);
            }
        }
        if !num_flac.is_undef() {
            // stop to process FLAC headers if we have them all
            if pl::le(&num_flac, &n(0)) {
                break;
            }
        } else if !val.get(stream.to_bytes()).is_undef() && flag.to_i64() & 0x04 != 0 {
            // process Ogg packet now if end-of-stream bit is set
            process_packet_impl(it, &et, &pl::elem_ref(&val, &stream.to_bytes()))?;
            val.delete(stream.to_bytes());
        }
    }
    if !num_flac.is_undef() && !val.get(stream.to_bytes()).is_undef() {
        // process FLAC headers as if it was a complete FLAC file
        crate::runtime::require(it, "Image::ExifTool::FLAC")?;
        let d = Value::new_hash();
        let r = pl::method(it, &pl::s("File::RandomAccess"), "new", vec![pl::elem_ref(&val, &stream.to_bytes())])?;
        d.set("RAF", r);
        pl::call(it, "Image::ExifTool::FLAC::ProcessFLAC", vec![et.clone(), d])?;
    }
    ret(success)
}

pub fn register(it: &mut Interp) {
    it.define_native("Image::ExifTool::Ogg::ProcessPacket", process_packet);
    it.define_native("Image::ExifTool::Ogg::ProcessOGG", process_ogg);
}
