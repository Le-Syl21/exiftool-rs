//! Image::ExifTool::H264 (Image/ExifTool/H264.pm), ported sub by sub (see docs/port/PORTING.md).
//!
//! Read meta information from H.264 video
//!
//! The BitStream object stays the Perl hash of the original (DataPt, Len,
//! Pos, Mask, Word). The NAL unit scan in ParseH264Video moves pos() on the
//! caller's `$$dataPt`: those matches run as Perl statements (`on_data`) so
//! pos() lives on that same scalar, as in perl.

use crate::interp::eval::ER;
use crate::interp::{Interp, Want};
use crate::modules::random_access::{cell, perl_eval};
use crate::perl::Value;
use crate::pl::{self, arg, n, ret};

const MODULE: &str = "Image::ExifTool::H264";

/// `Image::ExifTool::NAME(ARGS)` (the functions imported with :DataAccess :Utils).
fn call(it: &mut Interp, name: &str, args: Vec<Value>) -> ER<Value> {
    pl::call(it, &format!("Image::ExifTool::{name}"), args)
}

fn m(it: &mut Interp, et: &Value, name: &str, args: Vec<Value>) -> ER<Value> {
    pl::method(it, et, name, args)
}

fn first(r: Vec<Value>) -> Value {
    r.into_iter().last().unwrap_or_default()
}

/// An unsigned integer as perl keeps it (IV when it fits, else UV).
fn uv(v: u64) -> Value {
    if v <= i64::MAX as u64 {
        Value::Int(v as i64)
    } else {
        Value::UInt(v)
    }
}

/// A value as perl's bit operators see it (UV).
fn bits(v: &Value) -> u64 {
    match v {
        Value::UInt(u) => *u,
        v => v.to_i64() as u64,
    }
}

/// A Perl statement on `$dataPt` (the caller's buffer reference), in list context.
fn on_data(it: &mut Interp, code: &str, data_pt: &Value, p: Option<&Value>) -> ER<Vec<Value>> {
    let c = cell(data_pt.clone());
    let pc = cell(p.cloned().unwrap_or_default());
    perl_eval(it, code, MODULE, &[("dataPt", &c), ("p", &pc)], Want::List)
}

fn parse_picture_timing_flag(it: &mut Interp) -> Value {
    crate::runtime::lex_scalar(it, MODULE, "parsePictureTiming").borrow().clone()
}

//==============================================================================
// Bitstream functions (used for H264 video)
//
// Member variables:
//   Mask    = mask for next bit to read (0 when all data has been read)
//   Pos     = byte offset of next word to read
//   Word    = current data word
//   Len     = total data length in bytes
//   DataPt  = data pointer
//..............................................................................

/// Read next word from bitstream
/// Inputs: 0) BitStream ref
/// Returns: true if there is more data (and updates
///          Mask, Pos and Word for first bit in next word)
pub fn read_next_word(_it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let bstr = arg(&a, 0);
    let pos = bstr.get("Pos");
    if pl::le(&pl::add(&pos, &n(4)), &bstr.get("Len")) {
        pl::at(604);
        bstr.set("Word", pl::unpack1(&format!("x{} N", pos.to_text()), &pl::sderef(&bstr.get("DataPt"))?)?);
        bstr.set("Mask", n(0x80000000));
        bstr.set("Pos", pl::add(&bstr.get("Pos"), &n(4)));
    } else if pl::lt(&pos, &bstr.get("Len")) {
        pl::at(608);
        let mut bytes = pl::unpack(&format!("x{} C*", pos.to_text()), &pl::sderef(&bstr.get("DataPt"))?)?;
        let mut word = if bytes.is_empty() { 0 } else { bits(&bytes.remove(0)) };
        let mut mask: u64 = 0x80;
        while !bytes.is_empty() {
            word = (word << 8) | bits(&bytes.remove(0));
            mask <<= 8;
        }
        bstr.set("Word", uv(word));
        bstr.set("Mask", uv(mask));
        bstr.set("Pos", bstr.get("Len"));
    } else {
        return ret(n(0));
    }
    ret(n(1))
}

fn read_next_word1(it: &mut Interp, bstr: &Value) -> ER<bool> {
    Ok(first(read_next_word(it, vec![bstr.clone()], Want::Scalar)?).truthy())
}

/// Create a new BitStream object
/// Inputs: 0) data ref
/// Returns: BitStream ref, or null if data is empty
pub fn new_bit_stream(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let data_pt = arg(&a, 0);
    let mut bstr = Value::new_hash();
    bstr.set("DataPt", data_pt.clone());
    pl::at(630);
    bstr.set("Len", pl::length(&pl::sderef(&data_pt)?));
    bstr.set("Pos", n(0));
    bstr.set("Mask", n(0));
    if !read_next_word1(it, &bstr)? {
        bstr = Value::Undef;
    }
    ret(bstr)
}

/// Get number of bits remaining in bit stream
/// Inputs: 0) BitStream ref
/// Returns: number of bits remaining
pub fn bits_left(_it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let bstr = arg(&a, 0);
    let mut nbits = 0i64;
    let mut mask = bits(&bstr.get("Mask"));
    while mask != 0 {
        nbits += 1;
        mask >>= 1;
    }
    ret(pl::add(&n(nbits), &pl::mul(&n(8), &pl::sub(&bstr.get("Len"), &bstr.get("Pos")))))
}

/// Get integer from bitstream
/// Inputs: 0) BitStream ref, 1) number of bits
/// Returns: integer (and increments position in bitstream)
pub fn get_int_n(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (bstr, nb) = (arg(&a, 0), arg(&a, 1));
    let mut nb = nb.to_i64();
    let mut val: u64 = 0;
    loop {
        // while ($bits--)
        let b = nb;
        nb -= 1;
        if b == 0 {
            break;
        }
        val = val.wrapping_shl(1);
        if bits(&bstr.get("Mask")) & bits(&bstr.get("Word")) != 0 {
            val = val.wrapping_add(1);
        }
        let mask = bits(&bstr.get("Mask")) >> 1;
        bstr.set("Mask", uv(mask));
        if mask != 0 {
            continue;
        }
        if !read_next_word1(it, &bstr)? {
            break;
        }
    }
    ret(uv(val))
}

fn get_int_n1(it: &mut Interp, bstr: &Value, nb: Value) -> ER<Value> {
    Ok(first(get_int_n(it, vec![bstr.clone(), nb], Want::Scalar)?))
}

/// Get Exp-Golomb integer from bitstream
/// Inputs: 0) BitStream ref
/// Returns: integer (and increments position in bitstream)
pub fn get_golomb(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let bstr = arg(&a, 0);
    // first, count the number of zero bits to get the integer bit width
    let mut count = 0i64;
    while bits(&bstr.get("Mask")) & bits(&bstr.get("Word")) == 0 {
        count += 1;
        let mask = bits(&bstr.get("Mask")) >> 1;
        bstr.set("Mask", uv(mask));
        if mask != 0 {
            continue;
        }
        if !read_next_word1(it, &bstr)? {
            break;
        }
    }
    // then return the adjusted integer
    ret(pl::sub(&get_int_n1(it, &bstr, n(count + 1))?, &n(1)))
}

fn get_golomb1(it: &mut Interp, bstr: &Value) -> ER<Value> {
    Ok(first(get_golomb(it, vec![bstr.clone()], Want::Scalar)?))
}

/// Get signed Exp-Golomb integer from bitstream
/// Inputs: 0) BitStream ref
/// Returns: integer (and increments position in bitstream)
pub fn get_golomb_s(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let bstr = arg(&a, 0);
    let val = pl::add(&get_golomb1(it, &bstr)?, &n(1));
    let half = uv(bits(&val) >> 1);
    ret(if bits(&val) & 1 != 0 { pl::neg(&half) } else { half })
}

// end bitstream functions
//==============================================================================

/// Decode H.264 scaling matrices
/// Inputs: 0) BitStream ref
/// Reference: http://ffmpeg.org/
pub fn decode_scaling_matrices(it: &mut Interp, a: Vec<Value>, want: Want) -> ER<Vec<Value>> {
    let bstr = arg(&a, 0);
    if get_int_n1(it, &bstr, n(1))?.truthy() {
        for i in 0..8 {
            let size = if i < 6 { 16 } else { 64 };
            if !get_int_n1(it, &bstr, n(1))?.truthy() {
                continue;
            }
            let (last, mut next) = (n(8), n(8));
            for j in 0..size {
                if next.truthy() {
                    let g = first(get_golomb_s(it, vec![bstr.clone()], Want::Scalar)?);
                    next = n(pl::add(&last, &g).to_i64() & 0xff);
                }
                if !(j != 0 || next.truthy()) {
                    break;
                }
            }
        }
    }
    pl::ret_nothing(want)
}

/// Parse H.264 sequence parameter set RBSP (ref 1)
/// Inputs: 0) ExifTool ref, 1) tag table ref, 2) data ref
/// Notes: All this just to get the image size!
pub fn parse_seq_param_set(it: &mut Interp, a: Vec<Value>, want: Want) -> ER<Vec<Value>> {
    let (et, tag_table_ptr, data_pt) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    // initialize our bitstream object
    let bstr = first(new_bit_stream(it, vec![data_pt], Want::Scalar)?);
    if !bstr.truthy() {
        return pl::ret_nothing(want);
    }
    let b = &bstr;
    let mut t;
    let mut nn;
    // the messy nature of H.264 encoding makes it difficult to use
    // data-driven structure parsing, so I code it explicitly (yuck!)
    t = get_int_n1(it, b, n(8))?; // profile_idc
    get_int_n1(it, b, n(16))?; // constraints and level_idc
    get_golomb1(it, b)?; // seq_parameter_set_id
    if pl::ge(&t, &n(100)) {
        // (ref b)
        t = get_golomb1(it, b)?; // chroma_format_idc
        if pl::num_eq(&t, &n(3)) {
            get_int_n1(it, b, n(1))?; // separate_colour_plane_flag
            let _ = n(12);
        } else {
            let _ = n(8);
        }
        get_golomb1(it, b)?; // bit_depth_luma_minus8
        get_golomb1(it, b)?; // bit_depth_chroma_minus8
        get_int_n1(it, b, n(1))?; // qpprime_y_zero_transform_bypass_flag
        decode_scaling_matrices(it, vec![bstr.clone()], Want::Void)?;
    }
    get_golomb1(it, b)?; // log2_max_frame_num_minus4
    t = get_golomb1(it, b)?; // pic_order_cnt_type
    if pl::num_eq(&t, &n(0)) {
        get_golomb1(it, b)?; // log2_max_pic_order_cnt_lsb_minus4
    } else if pl::num_eq(&t, &n(1)) {
        get_int_n1(it, b, n(1))?; // delta_pic_order_always_zero_flag
        get_golomb1(it, b)?; // offset_for_non_ref_pic
        get_golomb1(it, b)?; // offset_for_top_to_bottom_field
        nn = get_golomb1(it, b)?; // num_ref_frames_in_pic_order_cnt_cycle
        let mut i = n(0);
        while pl::lt(&i, &nn) {
            get_golomb1(it, b)?; // offset_for_ref_frame[i]
            i = pl::add(&i, &n(1));
        }
    }
    get_golomb1(it, b)?; // num_ref_frames
    get_int_n1(it, b, n(1))?; // gaps_in_frame_num_value_allowed_flag
    let mut w = get_golomb1(it, b)?; // pic_width_in_mbs_minus1
    let mut h = get_golomb1(it, b)?; // pic_height_in_map_units_minus1
    let f = get_int_n1(it, b, n(1))?; // frame_mbs_only_flag
    if !f.truthy() {
        get_int_n1(it, b, n(1))?; // mb_adaptive_frame_field_flag
    }
    get_int_n1(it, b, n(1))?; // direct_8x8_inference_flag
    // convert image size to pixels
    w = pl::mul(&pl::add(&w, &n(1)), &n(16));
    h = pl::mul(&pl::mul(&pl::sub(&n(2), &f), &pl::add(&h, &n(1))), &n(16));
    // account for cropping (if any)
    t = get_int_n1(it, b, n(1))?; // frame_cropping_flag
    if t.truthy() {
        let mm = pl::sub(&n(4), &pl::mul(&f, &n(2)));
        w = pl::sub(&w, &pl::mul(&n(4), &get_golomb1(it, b)?)); // frame_crop_left_offset
        w = pl::sub(&w, &pl::mul(&n(4), &get_golomb1(it, b)?)); // frame_crop_right_offset
        h = pl::sub(&h, &pl::mul(&mm, &get_golomb1(it, b)?)); // frame_crop_top_offset
        h = pl::sub(&h, &pl::mul(&mm, &get_golomb1(it, b)?)); // frame_crop_bottom_offset
    }
    // quick validity checks (just in case)
    if !bstr.get("Mask").truthy() {
        return pl::ret_nothing(want);
    }
    if pl::ge(&w, &n(160)) && pl::le(&w, &n(4096)) && pl::ge(&h, &n(120)) && pl::le(&h, &n(3072)) {
        m(it, &et, "HandleTag", vec![tag_table_ptr.clone(), pl::s("ImageWidth"), w])?;
        m(it, &et, "HandleTag", vec![tag_table_ptr.clone(), pl::s("ImageHeight"), h])?;
        // (whew! -- so much work just to get ImageSize!!)
    }
    // return now unless interested in picture timing information
    if !parse_picture_timing_flag(it).truthy() {
        return pl::ret_nothing(want);
    }

    // parse vui parameters if they exist
    if !get_int_n1(it, b, n(1))?.truthy() {
        return pl::ret_nothing(want); // vui_parameters_present_flag
    }
    t = get_int_n1(it, b, n(1))?; // aspect_ratio_info_present_flag
    if t.truthy() {
        t = get_int_n1(it, b, n(8))?; // aspect_ratio_idc
        if pl::num_eq(&t, &n(255)) {
            // Extended_SAR ?
            get_int_n1(it, b, n(32))?; // sar_width/sar_height
        }
    }
    t = get_int_n1(it, b, n(1))?; // overscan_info_present_flag
    if t.truthy() {
        get_int_n1(it, b, n(1))?; // overscan_appropriate_flag
    }
    t = get_int_n1(it, b, n(1))?; // video_signal_type_present_flag
    if t.truthy() {
        get_int_n1(it, b, n(4))?; // video_format/video_full_range_flag
        t = get_int_n1(it, b, n(1))?; // colour_description_present_flag
        if t.truthy() {
            get_int_n1(it, b, n(24))?; // colour_primaries/transfer_characteristics/matrix_coefficients
        }
    }
    t = get_int_n1(it, b, n(1))?; // chroma_loc_info_present_flag
    if t.truthy() {
        get_golomb1(it, b)?; // chroma_sample_loc_type_top_field
        get_golomb1(it, b)?; // chroma_sample_loc_type_bottom_field
    }
    t = get_int_n1(it, b, n(1))?; // timing_info_present_flag
    if t.truthy() {
        if pl::lt(&first(bits_left(it, vec![bstr.clone()], Want::Scalar)?), &n(65)) {
            return pl::ret_nothing(want);
        }
        et.set("VUI_units", get_int_n1(it, b, n(32))?); // num_units_in_tick
        et.set("VUI_scale", get_int_n1(it, b, n(32))?); // time_scale
        get_int_n1(it, b, n(1))?; // fixed_frame_rate_flag
    }
    let mut hard = false;
    for _j in 0..2 {
        t = get_int_n1(it, b, n(1))?; // nal_/vcl_hrd_parameters_present_flag
        if t.truthy() {
            et.set("VUI_hard", n(1));
            hard = true;
            nn = get_golomb1(it, b)?; // cpb_cnt_minus1
            get_int_n1(it, b, n(8))?; // bit_rate_scale/cpb_size_scale
            let mut i = n(0);
            while pl::le(&i, &nn) {
                get_golomb1(it, b)?; // bit_rate_value_minus1[SchedSelIdx]
                get_golomb1(it, b)?; // cpb_size_value_minus1[SchedSelIdx]
                get_int_n1(it, b, n(1))?; // cbr_flag[SchedSelIdx]
                i = pl::add(&i, &n(1));
            }
            get_int_n1(it, b, n(5))?; // initial_cpb_removal_delay_length_minus1
            et.set("VUI_clen", get_int_n1(it, b, n(5))?); // cpb_removal_delay_length_minus1
            et.set("VUI_dlen", get_int_n1(it, b, n(5))?); // dpb_output_delay_length_minus1
            et.set("VUI_toff", get_int_n1(it, b, n(5))?); // time_offset_length
        }
    }
    if hard {
        get_int_n1(it, b, n(1))?; // low_delay_hrd_flag
    }
    let p = get_int_n1(it, b, n(1))?; // pic_struct_present_flag
    et.set("VUI_pic", p.clone());
    // (don't yet decode the rest of the vui data)
    ret(p)
}

/// Parse H.264 picture timing SEI message (payload type 1) (ref 1)
/// Inputs: 0) ExifTool ref, 1) data ref
/// Notes: this routine is for test purposes only, and not called unless the
///        $parsePictureTiming flag is set
pub fn parse_picture_timing(it: &mut Interp, a: Vec<Value>, want: Want) -> ER<Vec<Value>> {
    let (et, data_pt) = (arg(&a, 0), arg(&a, 1));
    let bstr = first(new_bit_stream(it, vec![data_pt], Want::Scalar)?);
    if !bstr.truthy() {
        return pl::ret_nothing(want);
    }
    let b = &bstr;
    let mut t;
    // the specification is very odd on this point: the following delays
    // exist if the VUI hardware parameters are present, or if
    // "determined by the application, by some means not specified" -- WTF??
    if et.get("VUI_hard").truthy() {
        get_int_n1(it, b, pl::add(&et.get("VUI_clen"), &n(1)))?; // cpb_removal_delay
        get_int_n1(it, b, pl::add(&et.get("VUI_dlen"), &n(1)))?; // dpb_output_delay
    }
    if et.get("VUI_pic").truthy() {
        t = get_int_n1(it, b, n(4))?; // pic_struct
        // determine NumClockTS ($n)
        let nn = match &*t.to_bytes() {
            b"0" | b"1" | b"2" => 1,
            b"3" | b"4" | b"7" => 2,
            b"5" | b"6" | b"8" => 3,
            _ => 0,
        };
        if nn == 0 {
            return pl::ret_nothing(want);
        }
        for _i in 0..nn {
            t = get_int_n1(it, b, n(1))?; // clock_timestamp_flag[i]
            if !t.truthy() {
                continue;
            }
            get_int_n1(it, b, n(2))?; // ct_type
            let _nu = get_int_n1(it, b, n(1))?; // nuit_field_based_flag
            get_int_n1(it, b, n(5))?; // counting_type
            t = get_int_n1(it, b, n(1))?; // full_timestamp_flag
            get_int_n1(it, b, n(1))?; // discontinuity_flag
            get_int_n1(it, b, n(1))?; // cnt_dropped_flag
            get_int_n1(it, b, n(8))?; // n_frames
            if t.truthy() {
                let _s = get_int_n1(it, b, n(6))?; // seconds_value
                let _m = get_int_n1(it, b, n(6))?; // minutes_value
                let _h = get_int_n1(it, b, n(5))?; // hours_value
            } else {
                t = get_int_n1(it, b, n(1))?; // seconds_flag
                if t.truthy() {
                    let _s = get_int_n1(it, b, n(6))?; // seconds_value
                    t = get_int_n1(it, b, n(1))?; // minutes_flag
                    if t.truthy() {
                        let _m = get_int_n1(it, b, n(6))?; // minutes_value
                        t = get_int_n1(it, b, n(1))?; // hours_flag
                        if t.truthy() {
                            let _h = get_int_n1(it, b, n(5))?; // hours_value
                        }
                    }
                }
            }
            if et.get("VUI_toff").truthy() {
                let _o = get_int_n1(it, b, et.get("VUI_toff"))?; // time_offset
            }
            break; // only parse the first clock timestamp found
        }
    }
    pl::ret_nothing(want)
}

/// Process H.264 Supplementary Enhancement Information (ref 1/PH)
/// Inputs: 0) ExifTool ref, 1) dirInfo ref, 2) tag table ref
/// Returns: 1 if we processed payload type 5
/// Payload types:
///   0 - buffer period
///   1 - pic timing
///   2 - pan scan rect
///   3 - filler payload
///   4 - user data registered itu t t35
///   5 - user data unregistered
///   6 - recovery point
///   7 - dec ref pic marking repetition
///   8 - spare pic
///   9 - sene info
///  10 - sub seq info
///  11 - sub seq layer characteristics
///  12 - sub seq characteristics
///  13 - full frame freeze
///  14 - full frame freeze release
///  15 - full frame snapshot
///  16 - progressive refinement segment start
///  17 - progressive refinement segment end
///  18 - motion constrained slice group set
pub fn process_sei(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info) = (arg(&a, 0), arg(&a, 1));
    let data_pt = dir_info.get("DataPt");
    pl::at(934);
    let data = pl::sderef(&data_pt)?;
    let mut end = pl::length(&data).to_i64();
    let mut pos: i64 = 0;
    let mut size: i64;
    let get8 = |it: &mut Interp, p: i64| -> ER<Value> { call(it, "Get8u", vec![data_pt.clone(), n(p)]) };

    // scan through SEI payload for type 5 (the unregistered user data)
    loop {
        let mut type_ = n(0);
        loop {
            if pos >= end {
                return ret(n(0));
            }
            let t = get8(it, pos)?; // payload type
            pos += 1;
            type_ = pl::add(&type_, &t);
            if !pl::num_eq(&t, &n(255)) {
                break;
            }
        }
        if pl::num_eq(&type_, &n(0x80)) {
            return ret(n(0)); // terminator (ref PH - maybe byte alignment bits?)
        }
        let mut sz = n(0);
        loop {
            if pos >= end {
                return ret(n(0));
            }
            let t = get8(it, pos)?; // payload data length
            pos += 1;
            sz = pl::add(&sz, &t);
            if !pl::num_eq(&t, &n(255)) {
                break;
            }
        }
        size = sz.to_i64();
        if pos + size > end {
            return ret(n(0));
        }
        m(it, &et, "VPrint", vec![n(1), pl::concat(&pl::concat(&pl::s("    (SEI type "), &type_), &pl::s(")\n"))])?;
        if pl::num_eq(&type_, &n(1)) {
            // picture timing information
            if parse_picture_timing_flag(it).truthy() {
                let _buff = pl::substr(&data, pos, Some(size));
                parse_picture_timing(it, vec![et.clone(), data_pt.clone()], Want::Void)?;
            }
        } else if pl::num_eq(&type_, &n(5)) {
            // unregistered user data
            break; // exit loop to process user data now
        }
        pos += size;
    }

    // look for our 16-byte UUID
    // - plus "MDPM" for "ModifiedDVPackMeta"
    // - plus "GA94" for closed-caption data (currently not decoded)
    let uuid: &[u8] = b"\x17\xee\x8c\x60\xf8\x4d\x11\xd9\x8c\xd6\x08\0\x20\x0c\x9a\x66MDPM";
    if !(size > 20 && pl::substr(&data, pos, Some(20)).to_bytes()[..] == *uuid) {
        return ret(n(0));
    }
    //
    // parse the MDPM records in the UUID 17ee8c60f84d11d98cd60800200c9a66
    // unregistered user data payload (ref PH)
    //
    let tag_table_ptr = call(it, "GetTagTable", vec![pl::s("Image::ExifTool::H264::MDPM")])?;
    let old_indent = et.get("INDENT");
    et.set("INDENT", pl::concat(&et.get("INDENT"), &pl::s("| ")));
    end = pos + size; // end of payload
    pos += 20; // skip UUID + "MDPM"
    let num = get8(it, pos)?; // get entry count
    pos += 1;
    let mut last_tag = n(0);
    if m(it, &et, "Options", vec![pl::s("Verbose")])?.truthy() {
        m(it, &et, "VerboseDir", vec![pl::s("MDPM"), num.clone()])?;
    }
    // walk through entries in the MDPM payload
    let mut index = n(0);
    while pl::lt(&index, &num) && pos < end {
        let tag = get8(it, pos)?;
        if pl::le(&tag, &last_tag) {
            // should be in numerical order (PH)
            m(it, &et, "Warn", vec![pl::s("Entries in MDPM directory are out of sequence")])?;
            break;
        }
        last_tag = tag.clone();
        let mut buff = pl::substr(&data, pos + 1, Some(4));
        let mut from = Value::Undef;
        let tag_info = m(it, &et, "GetTagInfo", vec![tag_table_ptr.clone(), tag.clone()])?;
        if tag_info.truthy() {
            // use our own print conversion for Unknown tags
            if tag_info.get("Unknown").truthy() && !tag_info.get("SetPrintConv").truthy() {
                tag_info.set("PrintConv", pl::s(r#"sprintf("0x%.8x", unpack("N", $val))"#));
                tag_info.set("SetPrintConv", n(1));
            }
            // combine with next value(s) if necessary
            let mut combine = tag_table_ptr.get_or_new_hash(tag.to_bytes()).get("Combine");
            while combine.truthy() {
                if pos + 5 >= end {
                    break;
                }
                let t = get8(it, pos + 5)?;
                if !pl::num_eq(&t, &pl::add(&last_tag, &n(1))) {
                    break; // must be consecutive tag ID's
                }
                pos += 5;
                buff = pl::concat(&buff, &pl::substr(&data, pos + 1, Some(4)));
                if from.is_undef() {
                    from = index.clone();
                }
                index = pl::add(&index, &n(1));
                last_tag = pl::add(&last_tag, &n(1));
                combine = pl::sub(&combine, &n(1));
            }
            let idx = if from.is_undef() { index.clone() } else { pl::concat(&pl::concat(&from, &pl::s("-")), &index) };
            let len = pl::length(&buff);
            m(
                it,
                &et,
                "HandleTag",
                vec![tag_table_ptr.clone(), tag, Value::Undef, pl::s("TagInfo"), tag_info, pl::s("DataPt"), pl::sref(buff), pl::s("Size"), len, pl::s("Index"), idx],
            )?;
        }
        pos += 5;
        index = pl::add(&index, &n(1));
    }
    et.set("INDENT", old_indent);
    ret(n(1))
}

/// Extract information from H.264 video stream
/// Inputs: 0) ExifTool ref, 1) data ref
/// Returns: 0 = done parsing, 1 = we want to parse more of these
pub fn parse_h264_video(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, data_pt) = (arg(&a, 0), arg(&a, 1));
    let verbose = m(it, &et, "Options", vec![pl::s("Verbose")])?;
    let out = m(it, &et, "Options", vec![pl::s("TextOut")])?;
    let tag_table_ptr = call(it, "GetTagTable", vec![pl::s("Image::ExifTool::H264::Main")])?;
    let mut parse_nal_unit: Vec<i64> = vec![0x06, 0x07]; // NAL unit types to parse
    let mut found_user_data = Value::Undef;
    pl::at(1040);
    let len = pl::length(&pl::sderef(&data_pt)?).to_i64();
    let mut pos: i64 = 0;
    while pos < len {
        let (next_pos, end);
        // find start of next NAL unit
        let r = on_data(it, r"$$dataPt =~ /(\0{2,3}\x01)/g ? (length($1), pos($$dataPt)) : ()", &data_pt, None)?;
        if !r.is_empty() {
            next_pos = arg(&r, 1).to_i64();
            end = next_pos - arg(&r, 0).to_i64();
            if pos == 0 {
                pos = next_pos;
                continue;
            }
        } else {
            if pos == 0 {
                break;
            }
            next_pos = len;
            end = len;
        }
        if pos >= len {
            break;
        }
        // parse NAL unit from $pos to $end
        let mut nal_unit_type = call(it, "Get8u", vec![data_pt.clone(), n(pos)])?.to_i64();
        pos += 1;
        // check forbidden_zero_bit
        if nal_unit_type & 0x80 != 0 {
            m(it, &et, "Warn", vec![pl::s("H264 forbidden bit error")])?;
            break;
        }
        nal_unit_type &= 0x1f;
        // ignore this NAL unit unless we will parse it
        if !(parse_nal_unit.contains(&nal_unit_type) || verbose.truthy()) {
            pos = next_pos;
            continue;
        }
        // read NAL unit (and convert all 0x000003's to 0x0000 as per spec.)
        let mut buff = pl::s("");
        on_data(it, "pos($$dataPt) = $p", &data_pt, Some(&n(pos + 1)))?;
        loop {
            let r = on_data(it, r"$$dataPt =~ /\0\0\x03/g ? (pos($$dataPt)) : ()", &data_pt, None)?;
            let Some(p) = r.into_iter().next() else { break };
            let p = p.to_i64();
            if p > end {
                break;
            }
            buff = pl::concat(&buff, &pl::substr(&data_pt.deref(), pos, Some(p - 1 - pos)));
            pos = p;
        }
        buff = pl::concat(&buff, &pl::substr(&data_pt.deref(), pos, Some(end - pos)));
        if pl::gt(&verbose, &n(1)) {
            let s = pl::sprintf("  NAL Unit Type: 0x%x (%d bytes)\n", &[n(nal_unit_type), pl::length(&buff)]);
            pl::print_to(it, Some(&out), &[s])?;
            m(it, &et, "VerboseDump", vec![pl::sref(buff.clone())])?;
        }
        pos = next_pos;
        on_data(it, "pos($$dataPt) = $p", &data_pt, Some(&n(pos)))?;

        if nal_unit_type == 0x06 {
            // sei_rbsp (supplemental enhancement info)

            if et.get("GotNAL06").truthy() {
                // process only the first SEI unless ExtractEmbedded is set
                if !m(it, &et, "Options", vec![pl::s("ExtractEmbedded")])?.truthy() {
                    continue;
                }
                et.set("DOC_NUM", et.get("GotNAL06"));
            }
            let di = Value::new_hash();
            di.set("DataPt", pl::sref(buff));
            found_user_data = first(process_sei(it, vec![et.clone(), di], Want::Scalar)?);
            et.delete("DOC_NUM");
            // keep parsing SEI's until we find the user data
            if !found_user_data.truthy() {
                continue;
            }
            let g = et.get("GotNAL06");
            et.set("GotNAL06", pl::add(&if g.truthy() { g } else { n(0) }, &n(1)));
        } else if nal_unit_type == 0x07 {
            // sequence_parameter_set_rbsp

            // process this NAL unit type only once
            if et.get("GotNAL07").truthy() {
                continue;
            }
            et.set("GotNAL07", n(1));
            parse_seq_param_set(it, vec![et.clone(), tag_table_ptr.clone(), pl::sref(buff)], Want::Void)?;
        }
        // we were successful, so don't parse this NAL unit type again
        parse_nal_unit.retain(|&x| x != nal_unit_type);
    }
    // parse one extra H264 frame if we didn't find the user data in this one
    // (Panasonic cameras don't put the SEI in the first frame)
    if found_user_data.truthy() || et.get("ParsedH264").truthy() {
        return ret(n(0));
    }
    et.set("ParsedH264", n(1));
    ret(n(1))
}

pub fn register(it: &mut Interp) {
    it.define_native("Image::ExifTool::H264::ReadNextWord", read_next_word);
    it.define_native("Image::ExifTool::H264::NewBitStream", new_bit_stream);
    it.define_native("Image::ExifTool::H264::BitsLeft", bits_left);
    it.define_native("Image::ExifTool::H264::GetIntN", get_int_n);
    it.define_native("Image::ExifTool::H264::GetGolomb", get_golomb);
    it.define_native("Image::ExifTool::H264::GetGolombS", get_golomb_s);
    it.define_native("Image::ExifTool::H264::DecodeScalingMatrices", decode_scaling_matrices);
    it.define_native("Image::ExifTool::H264::ParseSeqParamSet", parse_seq_param_set);
    it.define_native("Image::ExifTool::H264::ParsePictureTiming", parse_picture_timing);
    it.define_native("Image::ExifTool::H264::ProcessSEI", process_sei);
    it.define_native("Image::ExifTool::H264::ParseH264Video", parse_h264_video);
}
