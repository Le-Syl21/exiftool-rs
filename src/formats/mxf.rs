//! MXF (Material Exchange Format) format reader.

use super::misc::mktag;
use crate::error::{Error, Result};
use crate::tag::Tag;
use crate::value::Value;

pub fn read_mxf(data: &[u8]) -> Result<Vec<Tag>> {
    // Look for MXF KLV start marker: 06 0e 2b 34
    let magic = b"\x06\x0e\x2b\x34";
    let start = data
        .windows(4)
        .position(|w| w == magic.as_ref())
        .ok_or_else(|| Error::InvalidData("not an MXF file".into()))?;

    let data = &data[start..];
    let mut tags: Vec<Tag> = Vec::new();
    let mut registry: std::collections::HashMap<[u8; 2], [u8; 16]> =
        std::collections::HashMap::new();

    let mut pos = 0;
    while pos + 17 <= data.len() {
        if &data[pos..pos + 4] != b"\x06\x0e\x2b\x34" {
            pos += 1;
            continue;
        }
        let key = &data[pos..pos + 16];

        // Parse BER length at pos+16
        let len_byte = data[pos + 16];
        let (val_len, ber_size) = if len_byte < 0x80 {
            (len_byte as usize, 1usize)
        } else {
            let n = (len_byte & 0x7f) as usize;
            if pos + 17 + n > data.len() {
                break;
            }
            let mut l = 0usize;
            for i in 0..n {
                l = (l << 8) | (data[pos + 17 + i] as usize);
            }
            (l, 1 + n)
        };
        let val_start = pos + 16 + ber_size;
        if val_start + val_len > data.len() {
            break;
        }
        let val = &data[val_start..val_start + val_len];

        // Header/Footer partition (0d0102010102xxxx): parse MXFVersion at offset 0
        if key[4] == 0x02 && key[5] == 0x05 && key[12] == 0x01 && key[13] == 0x02 {
            if val.len() >= 4 && !tags.iter().any(|t| t.name == "MXFVersion") {
                let major = u16::from_be_bytes([val[0], val[1]]);
                let minor = u16::from_be_bytes([val[2], val[3]]);
                tags.push(mktag(
                    "MXF",
                    "MXFVersion",
                    "MXF Version",
                    Value::String(format!("{}.{}", major, minor)),
                ));
            }
        }
        // Primer Pack: key[12]=0x01 && key[13]=0x05 (0d010201010501xx)
        else if key[4] == 0x02 && key[5] == 0x05 && key[12] == 0x01 && key[13] == 0x05 {
            if val.len() >= 8 {
                let count = u32::from_be_bytes([val[0], val[1], val[2], val[3]]) as usize;
                let item_size = u32::from_be_bytes([val[4], val[5], val[6], val[7]]) as usize;
                if item_size >= 18 {
                    for i in 0..count {
                        let off = 8 + i * item_size;
                        if off + 18 > val.len() {
                            break;
                        }
                        let mut ltag = [0u8; 2];
                        ltag.copy_from_slice(&val[off..off + 2]);
                        let mut ul = [0u8; 16];
                        ul.copy_from_slice(&val[off + 2..off + 18]);
                        registry.insert(ltag, ul);
                    }
                }
            }
        }
        // Local Set (02 53): parse with primer registry
        else if key[4] == 0x02 && key[5] == 0x53 {
            mxf_parse_local_set(val, &registry, &mut tags);
        }

        pos = val_start + val_len;
    }

    // Deduplicate (keep last occurrence, like Perl's MXF dedup behavior)
    let mut last_index: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    for (i, t) in tags.iter().enumerate() {
        last_index.insert(t.name.clone(), i);
    }
    let mut result = Vec::new();
    for (i, t) in tags.into_iter().enumerate() {
        if last_index.get(&t.name) == Some(&i) {
            result.push(t);
        }
    }
    tags = result;

    Ok(tags)
}

fn mxf_parse_local_set(
    data: &[u8],
    registry: &std::collections::HashMap<[u8; 2], [u8; 16]>,
    tags: &mut Vec<Tag>,
) {
    let mut pos = 0;
    while pos + 4 <= data.len() {
        let ltag = [data[pos], data[pos + 1]];
        let llen = u16::from_be_bytes([data[pos + 2], data[pos + 3]]) as usize;
        pos += 4;
        if pos + llen > data.len() {
            break;
        }
        let val = &data[pos..pos + llen];

        if let Some(ul) = registry.get(&ltag) {
            let ul_hex: String = ul.iter().map(|b| format!("{:02x}", b)).collect();
            if let Some((name, value)) = mxf_decode_tag(&ul_hex, val) {
                tags.push(mktag("MXF", &name, &name, Value::String(value)));
            }
        }

        pos += llen;
    }
}

fn mxf_decode_tag(ul: &str, val: &[u8]) -> Option<(String, String)> {
    match ul {
        // Timestamps (8 bytes)
        "060e2b34010101020702011002040000" => {
            Some(("ContainerLastModifyDate".into(), mxf_decode_timestamp(val)))
        }
        "060e2b34010101020702011002030000" => {
            Some(("ModifyDate".into(), mxf_decode_timestamp(val)))
        }
        "060e2b34010101020702011001030000" => {
            Some(("CreateDate".into(), mxf_decode_timestamp(val)))
        }
        "060e2b34010101020702011002050000" => {
            Some(("PackageLastModifyDate".into(), mxf_decode_timestamp(val)))
        }
        // VersionType: 2 bytes major.minor
        "060e2b34010101020301020105000000" => {
            Some(("SDKVersion".into(), mxf_decode_version_short(val)))
        }
        // ProductVersion: 5 × int16u
        "060e2b3401010102052007010a000000" => {
            Some(("ToolkitVersion".into(), mxf_decode_product_version(val)))
        }
        // UTF-16 strings
        "060e2b34010101020520070102010000" => {
            Some(("ApplicationSupplierName".into(), mxf_decode_utf16(val)))
        }
        "060e2b34010101020520070103010000" => {
            Some(("ApplicationName".into(), mxf_decode_utf16(val)))
        }
        "060e2b34010101020520070105010000" => {
            Some(("ApplicationVersionString".into(), mxf_decode_utf16(val)))
        }
        "060e2b34010101020520070106010000" => {
            Some(("ApplicationPlatform".into(), mxf_decode_utf16(val)))
        }
        // TrackName (UTF-16)
        "060e2b34010101020107010201000000" => Some(("TrackName".into(), mxf_decode_utf16(val))),
        // TrackNumber (int32u)
        "060e2b34010101020104010300000000" => {
            if val.len() >= 4 {
                let n = u32::from_be_bytes([val[0], val[1], val[2], val[3]]);
                Some(("TrackNumber".into(), n.to_string()))
            } else {
                None
            }
        }
        // TrackID (int32u) - 060e2b34.0101.0102.01070101.00000000
        "060e2b34010101020107010100000000" => {
            if val.len() >= 4 {
                let n = u32::from_be_bytes([val[0], val[1], val[2], val[3]]);
                Some(("TrackID".into(), n.to_string()))
            } else {
                None
            }
        }
        // EditRate (rational64s)
        "060e2b34010101020530040500000000" => {
            if val.len() >= 8 {
                let num = i32::from_be_bytes([val[0], val[1], val[2], val[3]]);
                let den = i32::from_be_bytes([val[4], val[5], val[6], val[7]]);
                let rate = if den != 0 && den != 1 {
                    let r = num as f64 / den as f64;
                    if r == r.floor() {
                        format!("{}", r as i64)
                    } else {
                        format!("{:.6}", r).trim_end_matches('0').to_string()
                    }
                } else {
                    num.to_string()
                };
                Some(("EditRate".into(), rate))
            } else {
                None
            }
        }
        // RoundedTimecodeTimebase (int16u)
        "060e2b34010101020404010102060000" => {
            if val.len() >= 2 {
                let n = u16::from_be_bytes([val[0], val[1]]);
                Some(("RoundedTimecodeTimebase".into(), n.to_string()))
            } else {
                None
            }
        }
        // DropFrame (Boolean)
        "060e2b34010101010404010105000000" => {
            if !val.is_empty() {
                Some((
                    "DropFrame".into(),
                    if val[0] != 0 { "True" } else { "False" }.into(),
                ))
            } else {
                None
            }
        }
        // StartTimecode (int64s, shown as "N s")
        "060e2b34010101020702010301050000" => {
            if val.len() >= 8 {
                let n = i64::from_be_bytes([
                    val[0], val[1], val[2], val[3], val[4], val[5], val[6], val[7],
                ]);
                Some(("StartTimecode".into(), format!("{} s", n)))
            } else {
                None
            }
        }
        // ComponentDataDefinition (WeakReference to DataDefinition UL)
        "060e2b34010101020601010401020000" => Some((
            "ComponentDataDefinition".into(),
            mxf_decode_component_def(val),
        )),
        // Origin (int64s duration in edit units)
        "060e2b34010101020702010301040000" => {
            if val.len() >= 8 {
                let n = i64::from_be_bytes([
                    val[0], val[1], val[2], val[3], val[4], val[5], val[6], val[7],
                ]);
                Some(("Origin".into(), format!("{} s", n)))
            } else {
                None
            }
        }
        // Duration (int64s)
        "060e2b34010101020702010301030000" | "060e2b34010101020702010302000000" => {
            if val.len() >= 8 {
                let n = i64::from_be_bytes([
                    val[0], val[1], val[2], val[3], val[4], val[5], val[6], val[7],
                ]);
                if n > 1000000000 {
                    return None;
                } // all 0xff sentinel
                Some(("Duration".into(), format!("{} s", n)))
            } else {
                None
            }
        }
        // EssenceLength (int64s)
        "060e2b34010101010406010200000000" => {
            if val.len() >= 8 {
                let n = i64::from_be_bytes([
                    val[0], val[1], val[2], val[3], val[4], val[5], val[6], val[7],
                ]);
                if n > 1000000000 {
                    return None;
                }
                Some(("EssenceLength".into(), format!("{} s", n)))
            } else {
                None
            }
        }
        // SampleRate (rational64s)
        "060e2b34010101010406010100000000" => {
            if val.len() >= 8 {
                let num = i32::from_be_bytes([val[0], val[1], val[2], val[3]]);
                let den = i32::from_be_bytes([val[4], val[5], val[6], val[7]]);
                let rate = if den != 0 && den != 1 {
                    let r = num as f64 / den as f64;
                    if r == r.floor() {
                        format!("{}", r as i64)
                    } else {
                        format!("{:.6}", r)
                    }
                } else {
                    num.to_string()
                };
                Some(("SampleRate".into(), rate))
            } else {
                None
            }
        }
        // ChannelCount (int32u)
        "060e2b34010101050402010104000000" => {
            if val.len() >= 4 {
                let n = u32::from_be_bytes([val[0], val[1], val[2], val[3]]);
                Some(("ChannelCount".into(), n.to_string()))
            } else if !val.is_empty() {
                Some(("ChannelCount".into(), val[0].to_string()))
            } else {
                None
            }
        }
        // AudioSampleRate (rational64s)
        "060e2b34010101050402030101010000" => {
            if val.len() >= 8 {
                let num = i32::from_be_bytes([val[0], val[1], val[2], val[3]]);
                let den = i32::from_be_bytes([val[4], val[5], val[6], val[7]]);
                let rate = if den != 0 && den != 1 {
                    let r = num as f64 / den as f64;
                    if r == r.floor() {
                        format!("{}", r as i64)
                    } else {
                        format!("{:.6}", r)
                    }
                } else {
                    num.to_string()
                };
                Some(("AudioSampleRate".into(), rate))
            } else {
                None
            }
        }
        // BlockAlign (int16u)
        "060e2b34010101050402030201000000" => {
            if val.len() >= 2 {
                let n = u16::from_be_bytes([val[0], val[1]]);
                Some(("BlockAlign".into(), n.to_string()))
            } else {
                None
            }
        }
        // AverageBytesPerSecond (int32u)
        "060e2b34010101050402030305000000" => {
            if val.len() >= 4 {
                let n = u32::from_be_bytes([val[0], val[1], val[2], val[3]]);
                Some(("AverageBytesPerSecond".into(), n.to_string()))
            } else {
                None
            }
        }
        // LockedIndicator (Boolean)
        "060e2b34010101040402030104000000" => {
            if !val.is_empty() {
                Some((
                    "LockedIndicator".into(),
                    if val[0] != 0 { "True" } else { "False" }.into(),
                ))
            } else {
                None
            }
        }
        // BitsPerAudioSample (int32u)
        "060e2b34010101040402030304000000" => {
            if val.len() >= 4 {
                let n = u32::from_be_bytes([val[0], val[1], val[2], val[3]]);
                Some(("BitsPerAudioSample".into(), n.to_string()))
            } else {
                None
            }
        }
        // LinkedTrackID (int32u)
        "060e2b34010101050601010305000000" => {
            if val.len() >= 4 {
                let n = u32::from_be_bytes([val[0], val[1], val[2], val[3]]);
                Some(("LinkedTrackID".into(), n.to_string()))
            } else {
                None
            }
        }
        // EssenceStreamID (int32u)
        "060e2b34010101040103040400000000" => {
            if val.len() >= 4 {
                let n = u32::from_be_bytes([val[0], val[1], val[2], val[3]]);
                Some(("EssenceStreamID".into(), n.to_string()))
            } else if val.len() >= 2 {
                let n = u16::from_be_bytes([val[0], val[1]]);
                Some(("EssenceStreamID".into(), n.to_string()))
            } else {
                None
            }
        }
        _ => None,
    }
}

fn mxf_decode_timestamp(val: &[u8]) -> String {
    if val.len() < 8 {
        return String::new();
    }
    let year = u16::from_be_bytes([val[0], val[1]]);
    let month = val[2];
    let day = val[3];
    let hour = val[4];
    let min = val[5];
    let sec = val[6];
    let msec = (val[7] as u32) * 4;
    format!(
        "{:04}:{:02}:{:02} {:02}:{:02}:{:02}.{:03}",
        year, month, day, hour, min, sec, msec
    )
}

fn mxf_decode_version_short(val: &[u8]) -> String {
    if val.len() < 2 {
        return String::new();
    }
    format!("{}.{}", val[0], val[1])
}

fn mxf_decode_product_version(val: &[u8]) -> String {
    if val.len() < 10 {
        return String::new();
    }
    let major = u16::from_be_bytes([val[0], val[1]]);
    let minor = u16::from_be_bytes([val[2], val[3]]);
    let patch = u16::from_be_bytes([val[4], val[5]]);
    let build = u16::from_be_bytes([val[6], val[7]]);
    let rel_type = u16::from_be_bytes([val[8], val[9]]);
    let rel_str = match rel_type {
        0 => "unknown".to_string(),
        1 => "released".to_string(),
        2 => "debug".to_string(),
        3 => "patched".to_string(),
        4 => "beta".to_string(),
        5 => "private build".to_string(),
        _ => format!("unknown {}", rel_type),
    };
    format!("{}.{}.{}.{} {}", major, minor, patch, build, rel_str)
}

fn mxf_decode_utf16(val: &[u8]) -> String {
    let chars: Vec<u16> = val
        .chunks(2)
        .filter(|c| c.len() == 2)
        .map(|c| u16::from_be_bytes([c[0], c[1]]))
        .collect();
    String::from_utf16_lossy(&chars)
        .trim_end_matches('\0')
        .to_string()
}

fn mxf_decode_component_def(val: &[u8]) -> String {
    if val.len() < 16 {
        return String::new();
    }
    let ul: String = val[..16].iter().map(|b| format!("{:02x}", b)).collect();
    match ul.as_str() {
        "060e2b34040101020d01030102060200" => "Sound Essence Track".to_string(),
        "060e2b34040101010103020100000000" => "Picture Essence Track".to_string(),
        "060e2b34040101010103020200000000" => "Sound Essence Track".to_string(),
        "060e2b34040101010103020300000000" => "Data Essence Track".to_string(),
        "060e2b34040101010103020400000000" => "Descriptive Metadata Track".to_string(),
        _ => ul,
    }
}
