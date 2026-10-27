//! Image::ExifTool::JPEGDigest (Image/ExifTool/JPEGDigest.pm), ported sub by sub (see docs/port/PORTING.md).
//!
//! Calculate JPEGDigest and JPEGQualityEstimate

use crate::interp::eval::ER;
use crate::interp::{Interp, Want};
use crate::perl::Value;
use crate::pl::{self, arg, ret};

fn s(x: &str) -> Value {
    Value::str(x)
}

const MODULE: &str = "Image::ExifTool::JPEGDigest";

// the quality thresholds of EstimateQuality: @hash and @sums for a color
// JPEG, then for a greyscale JPEG
const COLOR_HASH: [i64; 100] = [
    1020, 1015, 932, 848, 780, 735, 702, 679, 660, 645,
    632, 623, 613, 607, 600, 594, 589, 585, 581, 571,
    555, 542, 529, 514, 494, 474, 457, 439, 424, 410,
    397, 386, 373, 364, 351, 341, 334, 324, 317, 309,
    299, 294, 287, 279, 274, 267, 262, 257, 251, 247,
    243, 237, 232, 227, 222, 217, 213, 207, 202, 198,
    192, 188, 183, 177, 173, 168, 163, 157, 153, 148,
    143, 139, 132, 128, 125, 119, 115, 108, 104, 99,
    94, 90, 84, 79, 74, 70, 64, 59, 55, 49,
    45, 40, 34, 30, 25, 20, 15, 11, 6, 4,
];
const COLOR_SUMS: [i64; 100] = [
    32640, 32635, 32266, 31495, 30665, 29804, 29146, 28599, 28104, 27670,
    27225, 26725, 26210, 25716, 25240, 24789, 24373, 23946, 23572, 22846,
    21801, 20842, 19949, 19121, 18386, 17651, 16998, 16349, 15800, 15247,
    14783, 14321, 13859, 13535, 13081, 12702, 12423, 12056, 11779, 11513,
    11135, 10955, 10676, 10392, 10208, 9928, 9747, 9564, 9369, 9193,
    9017, 8822, 8639, 8458, 8270, 8084, 7896, 7710, 7527, 7347,
    7156, 6977, 6788, 6607, 6422, 6236, 6054, 5867, 5684, 5495,
    5305, 5128, 4945, 4751, 4638, 4442, 4248, 4065, 3888, 3698,
    3509, 3326, 3139, 2957, 2775, 2586, 2405, 2216, 2037, 1846,
    1666, 1483, 1297, 1109, 927, 735, 554, 375, 201, 128,
];
const GREY_HASH: [i64; 100] = [
    510, 505, 422, 380, 355, 338, 326, 318, 311, 305,
    300, 297, 293, 291, 288, 286, 284, 283, 281, 280,
    279, 278, 277, 273, 262, 251, 243, 233, 225, 218,
    211, 205, 198, 193, 186, 181, 177, 172, 168, 164,
    158, 156, 152, 148, 145, 142, 139, 136, 133, 131,
    129, 126, 123, 120, 118, 115, 113, 110, 107, 105,
    102, 100, 97, 94, 92, 89, 87, 83, 81, 79,
    76, 74, 70, 68, 66, 63, 61, 57, 55, 52,
    50, 48, 44, 42, 39, 37, 34, 31, 29, 26,
    24, 21, 18, 16, 13, 11, 8, 6, 3, 2,
];
const GREY_SUMS: [i64; 100] = [
    16320, 16315, 15946, 15277, 14655, 14073, 13623, 13230, 12859, 12560,
    12240, 11861, 11456, 11081, 10714, 10360, 10027, 9679, 9368, 9056,
    8680, 8331, 7995, 7668, 7376, 7084, 6823, 6562, 6345, 6125,
    5939, 5756, 5571, 5421, 5240, 5086, 4976, 4829, 4719, 4616,
    4463, 4393, 4280, 4166, 4092, 3980, 3909, 3835, 3755, 3688,
    3621, 3541, 3467, 3396, 3323, 3247, 3170, 3096, 3021, 2952,
    2874, 2804, 2727, 2657, 2583, 2509, 2437, 2362, 2290, 2211,
    2136, 2068, 1996, 1915, 1858, 1773, 1692, 1620, 1552, 1477,
    1398, 1326, 1251, 1179, 1109, 1031, 961, 884, 814, 736,
    667, 592, 518, 441, 369, 292, 221, 151, 86, 64,
];

/// Estimate JPEG quality from quantization tables (ref 3)
/// Inputs: 0) 1) DQT segments array ref
/// Returns: JPEG quality, or undefined if it can't be calculated
/// (the table entries are bytes from unpack 'C', so the sums are plain integers)
pub fn estimate_quality(_it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let dqt_list = arg(&a, 0);
    let mut qtbl: Vec<Vec<i64>> = Vec::new();
    let mut quality = Value::Undef;

    // unpack DQT segments and sum quantization tables
    let mut sum: i64 = 0;
    'dqt: for dqt in dqt_list.items() {
        if dqt.is_undef() {
            continue;
        }
        let dlen = dqt.to_bytes().len() as i64;
        let mut i: i64 = 1;
        while i + 64 <= dlen {
            let qt: Vec<i64> = pl::unpack(&format!("x{i} C64"), &dqt)?.iter().map(|v| v.to_i64()).collect();
            for x in &qt {
                sum += x;
            }
            qtbl.push(qt);
            if qtbl.len() >= 4 {
                break 'dqt;
            }
            i += 65;
        }
    }
    if qtbl.is_empty() {
        return pl::ret_undef();
    }
    let at = |t: &Vec<i64>, k: usize| t.get(k).copied().unwrap_or(0);

    let mut qval = at(&qtbl[0], 2) + at(&qtbl[0], 53);
    let (hash, sums) = if qtbl.len() > 1 {
        // color JPEG
        qval += at(&qtbl[1], 0) + at(&qtbl[1], 63);
        (&COLOR_HASH, &COLOR_SUMS)
    } else {
        // greyscale JPEG
        (&GREY_HASH, &GREY_SUMS)
    };
    for i in 0..100 {
        if qval < hash[i] && sum < sums[i] {
            continue;
        }
        if (qval <= hash[i] && sum <= sums[i]) || i >= 50 {
            quality = Value::Int(i as i64 + 1);
        }
        break;
    }
    ret(quality)
}

/// Calculate JPEGDigest and/or JPEGQualityEstimate
/// Inputs: 0) ExifTool object ref, 1) DQT segments array ref, 2) subsampling string
pub fn calculate(it: &mut Interp, a: Vec<Value>, want: Want) -> ER<Vec<Value>> {
    let (et, dqt_list, sub_sampling) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));

    // estimate JPEG quality if requested
    let ra = et.get("OPTIONS").get("RequestAll");
    let all = ra.truthy() && pl::gt(&ra, &Value::Int(2));
    if all || et.get("REQ_TAG_LOOKUP").get("jpegqualityestimate").truthy() {
        let mut quality = estimate_quality(it, vec![dqt_list.clone()], Want::Scalar)?.pop().unwrap_or_default();
        if quality.is_undef() {
            quality = s("<unknown>");
        }
        pl::method(it, &et, "FoundTag", vec![s("JPEGQualityEstimate"), quality])?;
    }
    if !((all || et.get("REQ_TAG_LOOKUP").get("jpegdigest").truthy()) && sub_sampling.truthy()) {
        return pl::ret_nothing(want);
    }

    if pl::eval_block(it, |it| crate::runtime::require(it, "Digest::MD5"))?.is_none() {
        pl::method(it, &et, "Warn", vec![s("Digest::MD5 must be installed to calculate JPEGDigest")])?;
        return pl::ret_nothing(want);
    }
    // create a string of DQT tables (in indexed order), separated by zero bytes
    let mut dqt: Vec<u8> = Vec::new();
    for dat in dqt_list.items() {
        if !dat.truthy() {
            continue;
        }
        if Value::bytes(&dqt).truthy() {
            dqt.push(0);
        }
        dqt.extend_from_slice(&dat.to_bytes());
    }
    // generate ASCII-hex string of DQT MD5 digest
    let digest = pl::call(it, "Digest::MD5::md5", vec![Value::bytes(&dqt)])?;
    let mut md5 = pl::unpack1("H*", &digest)?;

    // add sub-sampling string unless we get a match without it
    let print_conv = crate::runtime::lex_hash(it, MODULE, "PrintConv");
    if !print_conv.get(md5.to_bytes()).truthy() {
        md5 = pl::concat(&pl::concat(&md5, &s(":")), &sub_sampling);
    }

    // add print conversion for JPEGDigest dynamically so it doesn't
    // bulk up the documentation and slow down loading unnecessarily
    let extra = pl::pkg_hash(it, "Image::ExifTool::Extra");
    extra.get_or_new_hash("JPEGDigest").set("PrintConv", print_conv);
    // (the sub returns the value of its last statement)
    let key = pl::method(it, &et, "FoundTag", vec![s("JPEGDigest"), md5])?;
    ret(key)
}

pub fn register(it: &mut Interp) {
    it.define_native("Image::ExifTool::JPEGDigest::EstimateQuality", estimate_quality);
    it.define_native("Image::ExifTool::JPEGDigest::Calculate", calculate);
}
