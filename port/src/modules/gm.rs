//! Image::ExifTool::GM (Image/ExifTool/GM.pm), ported sub by sub (see docs/port/PORTING.md).
//!
//! Read GM PDR data from automobile videos. The file's `my` variables
//! (%convertUnits, %changeScale, %changeOffset, %printConv, @channel,
//! %channelStruct) stay Perl data, read with `lex_hash` / `lex_array`.

use crate::interp::eval::{die, ER};
use crate::interp::{Interp, Want};
use crate::perl::ops;
use crate::perl::Value;
use crate::pl::{self, arg, n, ret};
use crate::runtime;

const MODULE: &str = "Image::ExifTool::GM";

fn s(x: &str) -> Value {
    pl::s(x)
}

/// String interpolation: the parts concatenated.
fn cat(parts: &[Value]) -> Value {
    let mut v = Vec::new();
    for p in parts {
        v.extend_from_slice(&p.to_bytes());
    }
    Value::bytes(v)
}

fn call(it: &mut Interp, name: &str, args: Vec<Value>) -> ER<Value> {
    pl::call(it, &format!("Image::ExifTool::{name}"), args)
}

/// The element index of `$$a[$i]` as an lvalue (negative from the end);
/// perl dies when a negative index is before the start of the array.
fn lv_index(arr: &Value, i: &Value) -> ER<usize> {
    let i = pl::int(i).to_i64();
    if i >= 0 {
        return Ok(i as usize);
    }
    let idx = arr.len() as i64 + i;
    if idx < 0 {
        return die(format!("Modification of non-creatable array value attempted, subscript {i}"));
    }
    Ok(idx as usize)
}

//------------------------------------------------------------------------------
// Print a CSV row
// Inputs: 0) ExifTool ref, 1) time stamp
pub fn print_csv(it: &mut Interp, a: Vec<Value>, want: Want) -> ER<Vec<Value>> {
    let (et, ts) = (arg(&a, 0), arg(&a, 1));
    let csv = et.get("GMCsv"); // get the list of channels with measurements
    if !csv.truthy() {
        return pl::ret_nothing(want);
    }
    if csv.is_empty() {
        return pl::ret_nothing(want);
    }
    let vals = et.get("GMVals");
    let gm_dict = et.get("GMDictionary");
    let items = Value::new_array(vec![s(""); gm_dict.len()]);
    let t = if ts.truthy() { ts } else { et.get("GMMaxTS") };
    items.set_at(0, pl::div(&t, &Value::Num(1e7))?);
    // fill in scaled measurements for this TimeStamp
    for c in csv.items() {
        let gm_chan = gm_dict.at(pl::int(&c).to_i64());
        let i = lv_index(&items, &c)?;
        let v = pl::add(&pl::mul(&vals.at(pl::int(&c).to_i64()), &gm_chan.get("Mult")), &gm_chan.get("Off"));
        items.set_at(i, v);
        // apply CSV conversion if applicable (ie. Gear)
        let conv = gm_chan.get("Conv");
        if !(conv.truthy() && !conv.get(items.at(i as i64).to_bytes()).is_undef()) {
            continue;
        }
        let cv = conv.get(items.at(i as i64).to_bytes());
        items.set_at(i, cv);
    }
    let out = et.path(&["OPTIONS", "TextOut"]);
    pl::print_to(it, Some(&out), &[pl::join(",", &items.items()), s("\n")])?;
    // clear the channel list
    if let Value::ArrayRef(r) = &csv {
        r.borrow_mut().clear();
    }
    pl::ret_nothing(want)
}

//------------------------------------------------------------------------------
// Process GM Marlin values ('mrlv' box)
// Inputs: 0) ExifTool object ref, 1) dirInfo ref, 2) tag table ref
// Returns: 1 on success
pub fn process_mrlv(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info, tag_table_ptr) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    let data_pt = dir_info.get("DataPt");
    let data_pos = dir_info.get("DataPos");
    let dir_len = pl::length(&data_pt.deref()).to_i64();
    let mut pos: i64 = 0;
    // data lengths for known formats
    let fmt_len = |f: &[u8]| -> i64 {
        match f {
            b"strs" => 64,
            b"lang" => 64,
            b"strl" => 256,
            b"time" => 32,
            b"date" => 32,
            b"tmzn" => 32,
            b"tstm" => 8,
            b"focc" => 4,
            b"kvp\0" => 64 + 256,
            _ => 0,
        }
    };
    pl::method(it, &et, "VerboseDir", vec![s("mrlv"), Value::Undef, n(dir_len)])?;
    while pos + 8 <= dir_len {
        let mut tag = pl::substr(&data_pt.deref(), pos, Some(4));
        let mut fmt = pl::substr(&data_pt.deref(), pos + 4, Some(4));
        let len = fmt_len(&fmt.to_bytes());
        if len == 0 {
            // (PrintableTagID isn't imported into this package: perl dies here too)
            tag = call(it, "GM::PrintableTagID", vec![tag])?;
            fmt = call(it, "GM::PrintableTagID", vec![fmt])?;
            pl::method(it, &et, "Warn", vec![cat(&[s("Unknown format ("), fmt, s(") for tag "), tag])])?;
            break;
        }
        if pos + 8 + len > dir_len {
            pl::method(it, &et, "Warn", vec![s("Truncated mrlv data")])?;
            break;
        }
        pl::method(
            it,
            &et,
            "HandleTag",
            vec![tag_table_ptr.clone(), tag, Value::Undef, s("DataPt"), data_pt.clone(), s("DataPos"), data_pos.clone(), s("Start"), n(pos + 8), s("Size"), n(len)],
        )?;
        pos += 8 + len;
    }
    ret(n(1))
}

//------------------------------------------------------------------------------
// Process GM Marlin dictionary ('mrld' box)
// Inputs: 0) ExifTool object ref, 1) dirInfo ref, 2) tag table ref
// Returns: 1 on success
pub fn process_mrld(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info, tag_table_ptr) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    let data_pt = dir_info.get("DataPt");
    let dir_len = pl::length(&data_pt.deref()).to_i64();
    let st = pl::method(it, &et, "Options", vec![s("Struct")])?;
    let struct_ = if st.truthy() { st } else { n(0) };
    let gm_dict = Value::new_array(Vec::new());
    et.set("GMDictionary", gm_dict.clone());
    let marl = call(it, "GetTagTable", vec![s("Image::ExifTool::GM::marl")])?;
    let mut csv = Value::Undef;
    let convert_units = runtime::lex_hash(it, MODULE, "convertUnits");
    let change_scale = runtime::lex_hash(it, MODULE, "changeScale");
    let change_offset = runtime::lex_hash(it, MODULE, "changeOffset");
    let print_conv = runtime::lex_hash(it, MODULE, "printConv");
    let channel = runtime::lex_array(it, MODULE, "channel");
    let channel_struct = runtime::lex_hash(it, MODULE, "channelStruct");

    pl::method(it, &et, "VerboseDir", vec![s("mrld"), Value::Undef, n(dir_len)])?;
    runtime::require(it, "Image/ExifTool/XMPStruct.pl")?;
    pl::call(it, "Image::ExifTool::XMP::AddFlattenedTags", vec![tag_table_ptr.clone(), Value::Undef, Value::Undef, n(1)])?;
    if pl::method(it, &et, "Options", vec![s("PrintCSV")])?.truthy() {
        csv = Value::new_array(Vec::new());
    }

    let mut pos: i64 = 0;
    while pos + 448 <= dir_len {
        // unpack 448-byte records:
        // 0. int32u - channel number
        // 1. int32u - measurement type
        // 2. int32u - units number
        // 3. string[64] - units string
        // 4. int32u - flags (0.visible, 1.linear conversion, 2.interpolation OK)
        // 5. int64u - interval
        // 6. int32s - min reading
        // 7. int32s - max reading
        // 8. double - disp min
        // 9. double - disp max
        // 10. double - multiplier
        // 11. double - offset
        // 12. string[64] - channel name
        // 13. string[64] - channel description
        let mut a = pl::unpack(&format!("x{pos}NNNZ64Na8N2a8a8a8a8Z64Z64"), &data_pt.deref())?;
        a.resize(14, Value::Undef);
        let cu = convert_units.get(a[3].to_bytes());
        let units = if cu.truthy() { cu } else { a[3].clone() };
        a[3] = pl::method(it, &et, "Decode", vec![a[3].clone(), s("UTF8")])?; // convert from UTF8
        // convert signed ints
        for i in [6, 7] {
            if ops::bit_and(&a[i], &n(0x8000000)).truthy() {
                a[i] = pl::sub(&a[i], &n(4294967296));
            }
        }
        // convert doubles
        for i in [8, 9, 10, 11] {
            a[i] = call(it, "GetDouble", vec![pl::sref(a[i].clone()), n(0)])?;
        }
        a[5] = call(it, "Get64u", vec![pl::sref(a[5].clone()), n(0)])?; // convert 64-bit int
        let chan = a[0].clone();
        let tag = pl::sprintf("Channel%.2d", &[chan.clone()]);
        let mut tag_info = tag_table_ptr.get(tag.to_bytes());
        let hash = Value::new_hash();
        for (i, v) in a.iter().enumerate().skip(1) {
            hash.set(channel.at(i as i64).to_bytes(), v.clone());
        }
        if !tag_info.truthy() {
            let ti = Value::new_hash();
            ti.set("Name", tag.clone());
            ti.set("Struct", channel_struct.clone());
            tag_info = call(it, "AddTagToTable", vec![tag_table_ptr.clone(), tag.clone(), ti])?;
            pl::call(it, "Image::ExifTool::XMP::AddFlattenedTags", vec![tag_table_ptr.clone(), tag.clone(), Value::Undef, n(1)])?;
        }
        // extract channel structure if specified
        if struct_.truthy() {
            let keys: Vec<Value> = channel.items().into_iter().skip(1).collect();
            hash.set("_ordered_keys_", Value::new_array(keys));
            pl::method(it, &et, "FoundTag", vec![tag_info.clone(), hash.clone()])?;
        }
        // extract flattened channel elements
        if pl::num_eq(&struct_, &n(0)) || pl::num_eq(&struct_, &n(2)) {
            for (i, v) in a.iter().enumerate().skip(1) {
                let t = pl::concat(&tag, &channel.at(i as i64));
                pl::method(it, &et, "HandleTag", vec![tag_table_ptr.clone(), t, v.clone()])?;
            }
        }
        // add corresponding tag to marl table
        let name = pl::call(it, "Image::ExifTool::MakeTagName", vec![a[12].clone()])?;
        tag_info = marl.get(name.to_bytes());
        if !tag_info.truthy() {
            pl::method(it, &et, "VPrint", vec![n(0), et.get("INDENT"), cat(&[s("[adding "), name.clone(), s("]\n")])])?;
            tag_info = call(it, "AddTagToTable", vec![marl.clone(), name.clone(), Value::new_hash()])?;
        }
        if !tag_info.get("Description").truthy() {
            tag_info.set("Description", a[13].clone());
        }
        if !tag_info.get("PrintConv").truthy() {
            // add a default print conversion
            let pc = print_conv.get(units.to_bytes());
            let pc = if pc.truthy() {
                pc
            } else {
                let mut b = b"\"$val ".to_vec();
                b.extend_from_slice(&crate::perl::regex::quotemeta(&units.to_bytes()));
                b.push(b'"');
                Value::bytes(b)
            };
            tag_info.set("PrintConv", pc);
        }
        // adjust multiplier/offset as necessary to scale to more appropriate units
        // (ie. to the units actually specified in this dictionary -- d'oh)
        let cs = change_scale.get(units.to_bytes());
        let cs = if cs.truthy() { cs } else { n(1) };
        let co = change_offset.get(units.to_bytes());
        let co = if co.truthy() { co } else { n(0) };
        let mult = pl::mul(&a[10], &cs);
        let off = pl::add(&pl::mul(&a[11], &cs), &co);
        let init = pl::int(&pl::div(&pl::add(&a[6], &a[7]), &n(2))?); // initial value for difference readings
        // save information about this channel necessary for processing the marl data
        let ch = Value::new_hash();
        ch.set("Name", name);
        ch.set("Mult", mult);
        ch.set("Off", off);
        ch.set("Init", init);
        let ci = lv_index(&gm_dict, &chan)?;
        gm_dict.set_at(ci, ch.clone());
        ch.set("Conv", tag_info.get("CSVConv"));
        if csv.truthy() {
            let v = pl::concat(&a[12], &if a[3].truthy() { cat(&[s(" ("), a[3].clone(), s(")")]) } else { s("") });
            let ci = lv_index(&csv, &chan)?;
            csv.set_at(ci, v);
        }
        pos += 448;
    }
    // channel 0 must not be defined because we use it for the TimeStamp
    if !gm_dict.at(0).is_undef() {
        pl::method(it, &et, "Warn", vec![s("Internal error: PDR channel 0 is used")])?;
        et.delete("GMDictionary");
    } else if csv.truthy() {
        csv.set_at(0, s("Time (s)"));
        for (i, v) in csv.items().into_iter().enumerate() {
            if v.is_undef() {
                csv.set_at(i, s(""));
            }
        }
        let out = et.path(&["OPTIONS", "TextOut"]);
        pl::print_to(it, Some(&out), &[pl::join(",", &csv.items()), s("\n")])?;
        et.set("GMCsv", Value::new_array(Vec::new()));
    }
    let code = Value::Code(it.sub_ref("Image::ExifTool::GM::PrintCSV"));
    pl::method(it, &et, "AddCleanup", vec![code])?; // print last CSV line when we are done
    // initialize variables for processing marl box
    et.set("GMVals", Value::new_array(Vec::new()));
    et.set("GMMaxTS", n(0));
    et.set("GMBadChan", n(0));
    ret(n(1))
}

//------------------------------------------------------------------------------
// Process GM 'marl' ctbx data (ref PH)
// Inputs: 0) ExifTool object ref, 1) dirInfo ref, 2) tag table ref
// Returns: 1 on success
// (see https://exiftool.org/forum/index.php?topic=11335.msg61393#msg61393)
pub fn process_marl(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info, tag_table_ptr) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    let data_pt = dir_info.get("DataPt");
    let or0 = |v: Value| if v.truthy() { v } else { n(0) };
    let data_pos = pl::add(&or0(dir_info.get("DataPos")), &or0(dir_info.get("Base")));
    let data_len = pl::length(&data_pt.deref()).to_i64();
    let mut vals = et.get("GMVals"); // running values for each channel (0=TimeStamp)
    let mut chan = et.get("GMChan"); // running channel number
    let gm_dict = et.get("GMDictionary");
    let csv = et.get("GMCsv");
    let mut max_ts = et.get("GMMaxTS");
    let req_gps_date_time = et.get("REQ_TAG_LOOKUP").get("gpsdatetime");
    let req_time_stamp = if req_gps_date_time.truthy() { et.get("REQ_TAG_LOOKUP").get("timestamp") } else { n(1) };
    let mut verbose2 = false;

    pl::method(it, &et, "VerboseDir", vec![s("marl"), Value::Undef, n(data_len)])?;
    if !gm_dict.truthy() {
        pl::method(it, &et, "Warn", vec![s("Missing marl dictionary")])?;
        return ret(n(0));
    }
    if pl::gt(&pl::method(it, &et, "Options", vec![s("Verbose")])?, &n(1)) {
        verbose2 = true;
    }
    if !vals.is_array() {
        vals = Value::new_array(Vec::new()); // (autovivified)
    }
    // (we use the 0th channel for the TimeStamp)
    if vals.at(0).is_undef() {
        vals.set_at(0, n(-1));
    }
    let mut ts = vals.at(0);
    let data = data_pt.deref();

    let mut pos: i64 = 0;
    while pos + 8 <= data_len {
        let a = pl::unpack(&format!("x{pos}NN"), &data)?;
        let (a0, a1) = (arg(&a, 0), arg(&a, 1));
        let ah = ops::shift_right(&a0, &n(24));
        let a2 = ops::bit_and(&ah, &n(0xc0));
        let val;
        let (mut chan_diff, mut val_diff) = (Value::Undef, Value::Undef);
        let mut tsv: Vec<Value> = Vec::new();
        let gm_chan;
        if pl::num_eq(&a2, &n(0xc0)) {
            // 16-byte full record?
            if pl::num_eq(&ah, &n(0xff)) {
                break; // exit at first empty record
            }
            chan = ops::bit_and(&a0, &n(0x0fffffff));
            gm_chan = gm_dict.at(pl::int(&chan).to_i64());
            if !gm_chan.truthy() {
                pos += 8;
                continue; // (shouldn't happen)
            }
            val = pl::sub(&a1, &n(if ops::bit_and(&a1, &n(0x80000000)).truthy() { 4294967296 } else { 0 }));
            let vi = lv_index(&vals, &chan)?;
            vals.set_at(vi, val.clone());
            if pos + 16 > data_len {
                break; // (shouldn't happen)
            }
            pos += 8; // point at time stamp
            tsv = pl::unpack(&format!("x{pos}NN"), &data)?;
            ts = pl::add(&pl::mul(&arg(&tsv, 0), &n(4294967296)), &arg(&tsv, 1));
        } else if pl::num_eq(&a2, &n(0x40)) {
            // 8-byte difference record?
            if chan.is_undef() {
                pos += 8;
                continue; // (shouldn't happen)
            }
            ts = pl::add(&ts, &a1); // increment time stamp
            chan_diff = pl::sub(&ops::bit_and(&ah, &n(0x3f)), &n(if ops::bit_and(&ah, &n(0x20)).truthy() { 0x40 } else { 0 }));
            chan = pl::add(&chan, &chan_diff); // increment the running channel number
            gm_chan = gm_dict.at(pl::int(&chan).to_i64());
            if !gm_chan.truthy() {
                pos += 8;
                continue; // (shouldn't happen)
            }
            let vi = lv_index(&vals, &chan)?;
            if vals.at(vi as i64).is_undef() {
                vals.set_at(vi, gm_chan.get("Init")); // init if necessary
            }
            val_diff = pl::sub(&ops::bit_and(&a0, &n(0x00ffffff)), &n(if ops::bit_and(&a0, &n(0x00800000)).truthy() { 0x01000000 } else { 0 }));
            val = pl::add(&vals.at(vi as i64), &val_diff); // increment the running value for this channel
            vals.set_at(vi, val.clone());
        } else {
            pos += 8;
            continue; // (shouldn't happen)
        }
        // ensure that the timestamps are monotonically increasing
        // (have seen backward steps up to 0.033 sec, so fudge these)
        if pl::gt(&ts, &max_ts) {
            if csv.truthy() {
                print_csv(it, vec![et.clone(), max_ts.clone()], Want::Void)?;
            } else {
                let dc = pl::add(&et.get("DOC_COUNT"), &n(1));
                et.set("DOC_COUNT", dc.clone());
                et.set("DOC_NUM", dc);
                if req_time_stamp.truthy() {
                    pl::method(it, &et, "HandleTag", vec![tag_table_ptr.clone(), s("TimeStamp"), ts.clone()])?;
                }
                if req_gps_date_time.truthy() {
                    pl::method(it, &et, "HandleTag", vec![tag_table_ptr.clone(), s("GPSDateTime"), ts.clone()])?;
                }
            }
            max_ts = ts.clone();
        }
        if csv.truthy() {
            csv.push(chan.clone());
            pos += 8;
            continue;
        }
        let scaled = pl::add(&pl::mul(&val, &gm_chan.get("Mult")), &gm_chan.get("Off"));
        pl::method(it, &et, "HandleTag", vec![tag_table_ptr.clone(), gm_chan.get("Name"), scaled.clone()])?;
        if verbose2 {
            let str_ = cat(&[s(" * "), gm_chan.get("Mult"), s(" + "), gm_chan.get("Off"), s(" = "), scaled]);
            let p0 = pl::sub(&pl::add(&data_pos, &n(pos)), &n(if pl::num_eq(&a2, &n(0xc0)) { 8 } else { 0 }));
            let (cd, vd) = if !tsv.is_empty() { (s(""), s("")) } else { (pl::sprintf("%+d", &[chan_diff.clone()]), pl::sprintf("%+d", &[val_diff.clone()])) };
            let fmt = cat(&[s("| %8.4x: %.8x %.8x chan"), cd, s("=%.2d "), gm_chan.get("Name"), vd, s(" = "), val.clone(), str_, s("\n")]);
            let line = pl::sprintf_v(&fmt, &[p0, a0.clone(), a1.clone(), chan.clone()]);
            pl::print_to(it, None, &[line])?;
            if !tsv.is_empty() {
                let line = pl::sprintf(
                    "| %8.4x: %.8x %.8x         TimeStamp = %.6f sec\n",
                    &[pl::add(&data_pos, &n(pos)), arg(&tsv, 0), arg(&tsv, 1), pl::div(&ts, &Value::Num(1e7))?],
                );
                pl::print_to(it, None, &[line])?;
            }
        }
        pos += 8;
    }
    vals.set_at(0, ts.clone()); // save last timestamp
    et.set("GMChan", chan); // save last channel number
    et.set("GMMaxTS", ts);
    et.delete("DOC_NUM");
    ret(n(1))
}

pub fn register(it: &mut Interp) {
    it.define_native("Image::ExifTool::GM::PrintCSV", print_csv);
    it.define_native("Image::ExifTool::GM::Process_mrlv", process_mrlv);
    it.define_native("Image::ExifTool::GM::Process_mrld", process_mrld);
    it.define_native("Image::ExifTool::GM::Process_marl", process_marl);
}
