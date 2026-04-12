//! DPX (Digital Picture Exchange) format reader.
//! Mirrors ExifTool's DPX.pm ProcessDPX.

use crate::error::{Error, Result};
use crate::tag::{Tag, TagGroup, TagId};
use crate::value::Value;

fn mk(name: &str, value: Value) -> Tag {
    let pv = value.to_display_string();
    Tag {
        id: TagId::Text(name.to_string()),
        name: name.to_string(),
        description: name.to_string(),
        group: TagGroup {
            family0: "File".into(),
            family1: "File".into(),
            family2: "Image".into(),
        },
        raw_value: value,
        print_value: pv,
        priority: 0,
    }
}

fn mk_print(name: &str, value: Value, print: String) -> Tag {
    Tag {
        id: TagId::Text(name.to_string()),
        name: name.to_string(),
        description: name.to_string(),
        group: TagGroup {
            family0: "File".into(),
            family1: "File".into(),
            family2: "Image".into(),
        },
        raw_value: value,
        print_value: print,
        priority: 0,
    }
}

fn read_str(data: &[u8], offset: usize, len: usize) -> String {
    if offset + len > data.len() { return String::new(); }
    let s = &data[offset..offset+len];
    // null-terminate
    let end = s.iter().position(|&b| b == 0).unwrap_or(len);
    crate::encoding::decode_utf8_or_latin1(&s[..end]).trim().to_string()
}

fn read_u32_be(data: &[u8], offset: usize) -> u32 {
    if offset + 4 > data.len() { return 0; }
    u32::from_be_bytes([data[offset], data[offset+1], data[offset+2], data[offset+3]])
}

fn read_u32_le(data: &[u8], offset: usize) -> u32 {
    if offset + 4 > data.len() { return 0; }
    u32::from_le_bytes([data[offset], data[offset+1], data[offset+2], data[offset+3]])
}

fn read_u16_be(data: &[u8], offset: usize) -> u16 {
    if offset + 2 > data.len() { return 0; }
    u16::from_be_bytes([data[offset], data[offset+1]])
}

fn read_u16_le(data: &[u8], offset: usize) -> u16 {
    if offset + 2 > data.len() { return 0; }
    u16::from_le_bytes([data[offset], data[offset+1]])
}

fn read_f32_be(data: &[u8], offset: usize) -> f32 {
    if offset + 4 > data.len() { return 0.0; }
    f32::from_bits(u32::from_be_bytes([data[offset], data[offset+1], data[offset+2], data[offset+3]]))
}

fn read_f32_le(data: &[u8], offset: usize) -> f32 {
    if offset + 4 > data.len() { return 0.0; }
    f32::from_bits(u32::from_le_bytes([data[offset], data[offset+1], data[offset+2], data[offset+3]]))
}

pub fn read_dpx(data: &[u8]) -> Result<Vec<Tag>> {
    if data.len() < 2080 {
        return Err(Error::InvalidData("DPX file too small".into()));
    }

    let big_endian = if data.starts_with(b"SDPX") {
        true
    } else if data.starts_with(b"XPDS") {
        false
    } else {
        return Err(Error::InvalidData("not a DPX file".into()));
    };

    let read_u32 = |off: usize| -> u32 {
        if big_endian { read_u32_be(data, off) } else { read_u32_le(data, off) }
    };
    let read_u16 = |off: usize| -> u16 {
        if big_endian { read_u16_be(data, off) } else { read_u16_le(data, off) }
    };
    let read_f32 = |off: usize| -> f32 {
        if big_endian { read_f32_be(data, off) } else { read_f32_le(data, off) }
    };

    let mut tags = Vec::new();

    // ByteOrder: offset 0
    let byte_order_str = if big_endian { "Big-endian" } else { "Little-endian" };
    tags.push(mk_print("ByteOrder", Value::String(byte_order_str.into()), byte_order_str.into()));

    // HeaderVersion: offset 8, string[8]
    let hdr_ver = read_str(data, 8, 8);
    if !hdr_ver.is_empty() {
        tags.push(mk("HeaderVersion", Value::String(hdr_ver)));
    }

    // DPXFileSize: offset 16, int32u
    let file_size = read_u32(16);
    tags.push(mk("DPXFileSize", Value::U32(file_size)));

    // DittoKey: offset 20, int32u
    let ditto_key = read_u32(20);
    let ditto_str = match ditto_key { 0 => "Same", 1 => "New", _ => "Unknown" };
    tags.push(mk_print("DittoKey", Value::U32(ditto_key), ditto_str.into()));

    // ImageFileName: offset 36, string[100]
    let img_fn = read_str(data, 36, 100);
    tags.push(mk("ImageFileName", Value::String(img_fn)));

    // CreateDate: offset 136, string[24]
    // Format: "YYYY:MM:DD:HH:MM:SS" -> convert to "YYYY:MM:DD HH:MM:SS"
    let create_date_raw = read_str(data, 136, 24);
    if !create_date_raw.is_empty() {
        let create_date = create_date_raw.replacen(':', " ", 3);
        // Only keep up to position 19 (YYYY:MM:DD HH:MM:SS)
        let create_date = if create_date.len() > 19 { create_date[..19].to_string() } else { create_date };
        tags.push(mk("CreateDate", Value::String(create_date)));
    }

    // Creator: offset 160, string[100]
    let creator = read_str(data, 160, 100);
    if !creator.is_empty() {
        tags.push(mk("Creator", Value::String(creator)));
    }

    // Project: offset 260, string[200]
    let project = read_str(data, 260, 200);
    if !project.is_empty() {
        tags.push(mk("Project", Value::String(project)));
    }

    // Copyright: offset 460, string[200]
    let copyright = read_str(data, 460, 200);
    if !copyright.is_empty() {
        tags.push(mk("Copyright", Value::String(copyright)));
    }

    // EncryptionKey: offset 660, int32u -> hex
    let enc_key = read_u32(660);
    let enc_key_str = format!("{:08x}", enc_key);
    tags.push(mk_print("EncryptionKey", Value::U32(enc_key), enc_key_str));

    // Orientation: offset 768, int16u
    if 768 + 2 <= data.len() {
        let orientation = read_u16(768);
        let orient_str = match orientation {
            0 => "Horizontal (normal)",
            1 => "Mirror vertical",
            2 => "Mirror horizontal",
            3 => "Rotate 180",
            4 => "Mirror horizontal and rotate 270 CW",
            5 => "Rotate 90 CW",
            6 => "Rotate 270 CW",
            7 => "Mirror horizontal and rotate 90 CW",
            _ => "Unknown",
        };
        tags.push(mk_print("Orientation", Value::U16(orientation), orient_str.into()));
    }

    // ImageElements: offset 770, int16u
    if 770 + 2 <= data.len() {
        let img_elements = read_u16(770);
        tags.push(mk("ImageElements", Value::U16(img_elements)));
    }

    // ImageWidth: offset 772, int32u
    if 772 + 4 <= data.len() {
        let width = read_u32(772);
        tags.push(mk("ImageWidth", Value::U32(width)));
    }

    // ImageHeight: offset 776, int32u
    if 776 + 4 <= data.len() {
        let height = read_u32(776);
        tags.push(mk("ImageHeight", Value::U32(height)));
    }

    // DataSign: offset 780, int32u
    if 780 + 4 <= data.len() {
        let data_sign = read_u32(780);
        let sign_str = match data_sign { 0 => "Unsigned", 1 => "Signed", _ => "Unknown" };
        tags.push(mk_print("DataSign", Value::U32(data_sign), sign_str.into()));
    }

    // ComponentsConfiguration: offset 800, int8u
    if 800 < data.len() {
        let comp_config = data[800];
        let comp_str = match comp_config {
            0 => "User-defined single component",
            1 => "Red (R)",
            2 => "Green (G)",
            3 => "Blue (B)",
            4 => "Alpha (matte)",
            6 => "Luminance (Y)",
            7 => "Chrominance (Cb, Cr, subsampled by two)",
            8 => "Depth (Z)",
            9 => "Composite video",
            50 => "R, G, B",
            51 => "R, G, B, Alpha",
            52 => "Alpha, B, G, R",
            100 => "Cb, Y, Cr, Y (4:2:2)",
            101 => "Cb, Y, A, Cr, Y, A (4:2:2:4)",
            102 => "Cb, Y, Cr (4:4:4)",
            103 => "Cb, Y, Cr, A (4:4:4:4)",
            150 => "User-defined 2 component element",
            151 => "User-defined 3 component element",
            152 => "User-defined 4 component element",
            153 => "User-defined 5 component element",
            154 => "User-defined 6 component element",
            155 => "User-defined 7 component element",
            156 => "User-defined 8 component element",
            _ => "Unknown",
        };
        tags.push(mk_print("ComponentsConfiguration", Value::U8(comp_config), comp_str.into()));
    }

    // TransferCharacteristic: offset 801, int8u
    if 801 < data.len() {
        let tc = data[801];
        let tc_str = match tc {
            0 => "User-defined",
            1 => "Printing density",
            2 => "Linear",
            3 => "Logarithmic",
            4 => "Unspecified video",
            5 => "SMPTE 274M",
            6 => "ITU-R 709-4",
            7 => "ITU-R 601-5 system B or G (625)",
            8 => "ITU-R 601-5 system M (525)",
            9 => "Composite video (NTSC)",
            10 => "Composite video (PAL)",
            11 => "Z (depth) - linear",
            12 => "Z (depth) - homogeneous",
            13 => "SMPTE ADX",
            14 => "ITU-R 2020 NCL",
            15 => "ITU-R 2020 CL",
            16 => "IEC 61966-2-4 xvYCC",
            17 => "ITU-R 2100 NCL/PQ",
            18 => "ITU-R 2100 ICtCp/PQ",
            19 => "ITU-R 2100 NCL/HLG",
            20 => "ITU-R 2100 ICtCp/HLG",
            21 => "RP 431-2:2011 Gama 2.6",
            22 => "IEC 61966-2-1 sRGB",
            _ => "Unknown",
        };
        tags.push(mk_print("TransferCharacteristic", Value::U8(tc), tc_str.into()));
    }

    // ColorimetricSpecification: offset 802, int8u
    if 802 < data.len() {
        let cs = data[802];
        let cs_str = match cs {
            0 => "User-defined",
            1 => "Printing density",
            4 => "Unspecified video",
            5 => "SMPTE 274M",
            6 => "ITU-R 709-4",
            7 => "ITU-R 601-5 system B or G (625)",
            8 => "ITU-R 601-5 system M (525)",
            9 => "Composite video (NTSC)",
            10 => "Composite video (PAL)",
            13 => "SMPTE ADX",
            14 => "ITU-R 2020",
            15 => "P3D65",
            16 => "P3DCI",
            17 => "P3D60",
            18 => "ACES",
            _ => "Unknown",
        };
        tags.push(mk_print("ColorimetricSpecification", Value::U8(cs), cs_str.into()));
    }

    // BitDepth: offset 803, int8u
    if 803 < data.len() {
        tags.push(mk("BitDepth", Value::U8(data[803])));
    }

    // ImageDescription: offset 820, string[32]
    if 820 + 32 <= data.len() {
        let img_desc = read_str(data, 820, 32);
        if !img_desc.is_empty() {
            tags.push(mk("ImageDescription", Value::String(img_desc)));
        }
    }

    // SourceFileName: offset 1432, string[100]
    if 1432 + 100 <= data.len() {
        let src_fn = read_str(data, 1432, 100);
        tags.push(mk("SourceFileName", Value::String(src_fn)));
    }

    // SourceCreateDate: offset 1532, string[24]
    if 1532 + 24 <= data.len() {
        let src_date = read_str(data, 1532, 24);
        tags.push(mk("SourceCreateDate", Value::String(src_date)));
    }

    // InputDeviceName: offset 1556, string[32]
    if 1556 + 32 <= data.len() {
        let dev_name = read_str(data, 1556, 32);
        if !dev_name.is_empty() {
            tags.push(mk("InputDeviceName", Value::String(dev_name)));
        }
    }

    // InputDeviceSerialNumber: offset 1588, string[32]
    if 1588 + 32 <= data.len() {
        let dev_serial = read_str(data, 1588, 32);
        if !dev_serial.is_empty() {
            tags.push(mk("InputDeviceSerialNumber", Value::String(dev_serial)));
        }
    }

    // OriginalFrameRate: offset 1724, float
    if 1724 + 4 <= data.len() {
        let ofr = read_f32(1724);
        if ofr.is_finite() {
            let ofr_val = if ofr == ofr.trunc() {
                format!("{}", ofr as u32)
            } else {
                format!("{}", ofr)
            };
            tags.push(mk_print("OriginalFrameRate", Value::F64(ofr as f64), ofr_val));
        }
    }

    // FrameID: offset 1732, string[32]
    if 1732 + 32 <= data.len() {
        let frame_id = read_str(data, 1732, 32);
        tags.push(mk("FrameID", Value::String(frame_id)));
    }

    // SlateInformation: offset 1764, string[100]
    if 1764 + 100 <= data.len() {
        let slate = read_str(data, 1764, 100);
        tags.push(mk("SlateInformation", Value::String(slate)));
    }

    // TimeCode: offset 1920, int32u
    if 1920 + 4 <= data.len() {
        let tc = read_u32(1920);
        tags.push(mk("TimeCode", Value::U32(tc)));
    }

    // UserID: offset 2048, string[32]
    if 2048 + 32 <= data.len() {
        let uid = read_str(data, 2048, 32);
        if !uid.is_empty() {
            tags.push(mk("UserID", Value::String(uid)));
        }
    }

    Ok(tags)
}
