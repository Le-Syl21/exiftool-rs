//! Apple Binary PLIST parser.
//! Reads binary property list format used in Apple MakerNotes (RunTime, etc.)

use std::collections::HashMap;

/// Parse a binary plist and return key-value pairs.
pub fn parse_binary_plist(data: &[u8]) -> Option<HashMap<String, PlistValue>> {
    if data.len() < 40 { return None; }

    // Check magic: "bplist00"
    if !data.starts_with(b"bplist0") { return None; }

    // Trailer: last 32 bytes
    let trailer = &data[data.len() - 32..];
    let _unused = &trailer[0..6];
    let int_size = trailer[6] as usize;
    let ref_size = trailer[7] as usize;
    let num_obj = u64::from_be_bytes([trailer[8], trailer[9], trailer[10], trailer[11],
        trailer[12], trailer[13], trailer[14], trailer[15]]) as usize;
    let top_obj = u64::from_be_bytes([trailer[16], trailer[17], trailer[18], trailer[19],
        trailer[20], trailer[21], trailer[22], trailer[23]]) as usize;
    let table_off = u64::from_be_bytes([trailer[24], trailer[25], trailer[26], trailer[27],
        trailer[28], trailer[29], trailer[30], trailer[31]]) as usize;

    if top_obj >= num_obj || int_size == 0 || ref_size == 0 { return None; }
    if table_off + int_size * num_obj > data.len() { return None; }

    // Read offset table
    let mut offsets = Vec::with_capacity(num_obj);
    for i in 0..num_obj {
        let off = read_int(data, table_off + i * int_size, int_size)?;
        offsets.push(off);
    }

    // Parse objects recursively starting from top_obj
    let result = parse_object(data, &offsets, ref_size, top_obj)?;

    // Convert to HashMap if it's a dict
    if let PlistValue::Dict(map) = result {
        Some(map)
    } else {
        None
    }
}

/// Plist value types.
#[derive(Debug, Clone)]
pub enum PlistValue {
    Int(i64),
    Real(f64),
    Bool(bool),
    String(String),
    Data(Vec<u8>),
    Dict(HashMap<String, PlistValue>),
    Array(Vec<PlistValue>),
    Null,
}

fn parse_object(data: &[u8], offsets: &[usize], ref_size: usize, idx: usize) -> Option<PlistValue> {
    if idx >= offsets.len() { return None; }
    let off = offsets[idx];
    if off >= data.len() { return None; }

    let marker = data[off];
    let obj_type = marker >> 4;
    let obj_info = (marker & 0x0F) as usize;

    match obj_type {
        0x0 => {
            // Singleton: null, bool, fill
            match obj_info {
                0 => Some(PlistValue::Null),
                8 => Some(PlistValue::Bool(false)),
                9 => Some(PlistValue::Bool(true)),
                _ => Some(PlistValue::Null),
            }
        }
        0x1 => {
            // Int: 2^obj_info bytes
            let size = 1 << obj_info;
            let val = read_int_signed(data, off + 1, size)?;
            Some(PlistValue::Int(val))
        }
        0x2 => {
            // Real: 2^obj_info bytes
            let size = 1 << obj_info;
            if size == 4 && off + 5 <= data.len() {
                let bits = u32::from_be_bytes([data[off+1], data[off+2], data[off+3], data[off+4]]);
                Some(PlistValue::Real(f32::from_bits(bits) as f64))
            } else if size == 8 && off + 9 <= data.len() {
                let bits = u64::from_be_bytes([data[off+1], data[off+2], data[off+3], data[off+4],
                    data[off+5], data[off+6], data[off+7], data[off+8]]);
                Some(PlistValue::Real(f64::from_bits(bits)))
            } else { None }
        }
        0x5 => {
            // ASCII string
            let len = if obj_info == 0x0F {
                let (l, _) = read_length(data, off + 1)?;
                l
            } else { obj_info };
            let start = if obj_info == 0x0F { off + 3 } else { off + 1 };
            if start + len > data.len() { return None; }
            Some(PlistValue::String(String::from_utf8_lossy(&data[start..start+len]).to_string()))
        }
        0x6 => {
            // UTF-16 string
            let len = if obj_info == 0x0F {
                let (l, _) = read_length(data, off + 1)?;
                l
            } else { obj_info };
            let start = if obj_info == 0x0F { off + 3 } else { off + 1 };
            let byte_len = len * 2;
            if start + byte_len > data.len() { return None; }
            let units: Vec<u16> = data[start..start+byte_len].chunks_exact(2)
                .map(|c| u16::from_be_bytes([c[0], c[1]])).collect();
            Some(PlistValue::String(String::from_utf16_lossy(&units)))
        }
        0xD => {
            // Dict
            let count = if obj_info == 0x0F {
                let (l, _) = read_length(data, off + 1)?;
                l
            } else { obj_info };
            let keys_start = if obj_info == 0x0F { off + 3 } else { off + 1 };
            let vals_start = keys_start + count * ref_size;

            let mut map = HashMap::new();
            for i in 0..count {
                let key_ref = read_int(data, keys_start + i * ref_size, ref_size)?;
                let val_ref = read_int(data, vals_start + i * ref_size, ref_size)?;
                if let Some(PlistValue::String(key)) = parse_object(data, offsets, ref_size, key_ref) {
                    if let Some(val) = parse_object(data, offsets, ref_size, val_ref) {
                        map.insert(key, val);
                    }
                }
            }
            Some(PlistValue::Dict(map))
        }
        _ => None,
    }
}

fn read_int(data: &[u8], off: usize, size: usize) -> Option<usize> {
    if off + size > data.len() { return None; }
    let mut val = 0usize;
    for i in 0..size { val = (val << 8) | data[off + i] as usize; }
    Some(val)
}

fn read_int_signed(data: &[u8], off: usize, size: usize) -> Option<i64> {
    if off + size > data.len() { return None; }
    let mut val = 0i64;
    for i in 0..size { val = (val << 8) | data[off + i] as i64; }
    Some(val)
}

fn read_length(data: &[u8], off: usize) -> Option<(usize, usize)> {
    if off >= data.len() { return None; }
    let marker = data[off];
    let size = 1 << (marker & 0x0F);
    let val = read_int(data, off + 1, size)?;
    Some((val, 1 + size))
}
