//! Nikon Capture data decoder.
//!
//! Decodes NikonCaptureData (MakerNote tag 0x0E01) which contains
//! Nikon Capture Editor settings in a tagged binary format.
//! Mirrors ExifTool's NikonCapture.pm.

use crate::tag::{Tag, TagGroup, TagId};
use crate::value::Value;

fn mk(name: &str, val: &str) -> Tag {
    Tag {
        id: TagId::Text(name.into()),
        name: name.into(),
        description: name.into(),
        group: TagGroup {
            family0: "MakerNotes".into(),
            family1: "NikonCapture".into(),
            family2: "Image".into(),
        },
        raw_value: Value::String(val.into()),
        print_value: val.into(),
        priority: 0,
    }
}

fn off_on(v: u8) -> &'static str {
    if v != 0 {
        "On"
    } else {
        "Off"
    }
}

fn no_yes(v: u8) -> &'static str {
    if v != 0 {
        "Yes"
    } else {
        "No"
    }
}

fn ru32(data: &[u8], off: usize) -> u32 {
    if off + 4 > data.len() {
        return 0;
    }
    u32::from_le_bytes([data[off], data[off + 1], data[off + 2], data[off + 3]])
}

fn ri32(data: &[u8], off: usize) -> i32 {
    if off + 4 > data.len() {
        return 0;
    }
    i32::from_le_bytes([data[off], data[off + 1], data[off + 2], data[off + 3]])
}

fn ru16(data: &[u8], off: usize) -> u16 {
    if off + 2 > data.len() {
        return 0;
    }
    u16::from_le_bytes([data[off], data[off + 1]])
}

fn rf64(data: &[u8], off: usize) -> f64 {
    if off + 8 > data.len() {
        return 0.0;
    }
    f64::from_le_bytes([
        data[off],
        data[off + 1],
        data[off + 2],
        data[off + 3],
        data[off + 4],
        data[off + 5],
        data[off + 6],
        data[off + 7],
    ])
}

pub fn decode_nikon_capture(data: &[u8]) -> Vec<Tag> {
    let mut tags = Vec::new();
    if data.len() < 22 {
        return tags;
    }

    let mut pos = 22; // skip 22-byte header

    while pos + 22 < data.len() {
        let tag_id = ru32(data, pos);
        let raw_size = ru32(data, pos + 18);
        if raw_size < 4 {
            break;
        }
        let size = (raw_size - 4) as usize;
        pos += 22;
        if pos + size > data.len() {
            break;
        }

        let entry_data = &data[pos..pos + size];
        decode_nikon_capture_tag(tag_id, entry_data, &mut tags);

        pos += size;
    }

    tags
}

fn decode_nikon_capture_tag(tag_id: u32, data: &[u8], tags: &mut Vec<Tag>) {
    match tag_id {
        // Simple on/off or value tags
        0x008ae85e => {
            if !data.is_empty() {
                tags.push(mk("LCHEditor", off_on(data[0])));
            }
        }
        0x0c89224b => {
            if !data.is_empty() {
                tags.push(mk("ColorAberrationControl", off_on(data[0])));
            }
        }
        0x2175eb78 => {
            if !data.is_empty() {
                tags.push(mk("D-LightingHQ", off_on(data[0])));
            }
        }
        0x2fc08431 => {
            if data.len() >= 8 {
                tags.push(mk("StraightenAngle", &format!("{}", rf64(data, 0))));
            }
        }
        0x416391c6 => {
            if !data.is_empty() {
                tags.push(mk("QuickFix", off_on(data[0])));
            }
        }
        0x5f0e7d23 => {
            if !data.is_empty() {
                tags.push(mk("ColorBooster", off_on(data[0])));
            }
        }
        0x6a6e36b6 => {
            if !data.is_empty() {
                tags.push(mk("D-LightingHQSelected", no_yes(data[0])));
            }
        }
        0x753dcbc0 => {
            if !data.is_empty() {
                tags.push(mk("NoiseReduction", off_on(data[0])));
            }
        }
        0x76a43200 => {
            if !data.is_empty() {
                tags.push(mk("UnsharpMask", off_on(data[0])));
            }
        }
        0x76a43201 => {
            if !data.is_empty() {
                tags.push(mk("Curves", off_on(data[0])));
            }
        }
        0x76a43202 => {
            if !data.is_empty() {
                tags.push(mk("ColorBalanceAdj", off_on(data[0])));
            }
        }
        0x76a43203 => {
            if !data.is_empty() {
                tags.push(mk("AdvancedRaw", off_on(data[0])));
            }
        }
        0x76a43204 => {
            if !data.is_empty() {
                tags.push(mk("WhiteBalanceAdj", off_on(data[0])));
            }
        }
        0x76a43205 => {
            if !data.is_empty() {
                tags.push(mk("VignetteControl", off_on(data[0])));
            }
        }
        0x76a43206 => {
            if !data.is_empty() {
                tags.push(mk("FlipHorizontal", no_yes(data[0])));
            }
        }
        0x76a43207 => {
            // Rotation — int16u
            if data.len() >= 2 {
                let v = u16::from_le_bytes([data[0], data[1]]);
                tags.push(mk("Rotation", &v.to_string()));
            }
        }
        0xab5eca5e => {
            if !data.is_empty() {
                tags.push(mk("PhotoEffects", off_on(data[0])));
            }
        }
        0xac6bd5c0 => {
            if data.len() >= 2 {
                let v = i16::from_le_bytes([data[0], data[1]]);
                tags.push(mk("VignetteControlIntensity", &v.to_string()));
            }
        }
        0xce5554aa => {
            if !data.is_empty() {
                tags.push(mk("D-LightingHS", off_on(data[0])));
            }
        }
        0xe2173c47 => {
            if !data.is_empty() {
                tags.push(mk("PictureControl", off_on(data[0])));
            }
        }
        0xfe28a44f => {
            if !data.is_empty() {
                tags.push(mk("AutoRedEye", off_on(data[0])));
            }
        }
        0xfe443a45 => {
            if !data.is_empty() {
                tags.push(mk("ImageDustOff", off_on(data[0])));
            }
        }

        // Sub-tables
        0xe42b5161 => decode_unsharp_data(data, tags),
        0x374233e0 => decode_crop_data(data, tags),
        0x56a54260 => decode_exposure(data, tags),
        0xe37b4337 => decode_dlighting_hs(data, tags),
        0x890ff591 => decode_dlighting_hq(data, tags),
        0xb999a36f => decode_color_boost(data, tags),
        0x926f13e0 => decode_noise_reduction(data, tags),
        0x84589434 => decode_brightness(data, tags),
        0xb0384e1e => decode_photo_effects(data, tags),
        0xbf3c6c20 => decode_wb_adj(data, tags),

        0x3cfc73c6 => {
            // RedEyeData subdirectory
            if !data.is_empty() {
                let v = match data[0] {
                    0 => "Off",
                    1 => "Automatic",
                    2 => "Click on Eyes",
                    _ => "",
                };
                if !v.is_empty() {
                    tags.push(mk("RedEyeCorrection", v));
                }
            }
        }

        // Edit version name
        0x3d136244 => {
            let s = crate::encoding::decode_utf8_or_latin1(data)
                .trim_end_matches('\0')
                .to_string();
            if !s.is_empty() {
                tags.push(mk("EditVersionName", &s));
            }
        }

        _ => {} // Unknown tags — skip
    }
}

fn decode_unsharp_data(data: &[u8], tags: &mut Vec<Tag>) {
    if data.is_empty() {
        return;
    }
    tags.push(mk("UnsharpCount", &data[0].to_string()));
    // Unsharp1: Color at 19 (int16u), Intensity at 23 (int16u), HaloWidth at 25 (int16u), Threshold at 27
    if data.len() > 19 {
        tags.push(mk("Unsharp1Color", unsharp_color(ru16(data, 19))));
    }
    if data.len() > 24 {
        tags.push(mk("Unsharp1Intensity", &ru16(data, 23).to_string()));
    }
    if data.len() > 26 {
        tags.push(mk("Unsharp1HaloWidth", &ru16(data, 25).to_string()));
    }
    if data.len() > 27 {
        tags.push(mk("Unsharp1Threshold", &data[27].to_string()));
    }
}

fn unsharp_color(v: u16) -> &'static str {
    match v {
        0 => "RGB",
        1 => "Red",
        2 => "Green",
        3 => "Blue",
        4 => "Yellow",
        5 => "Magenta",
        6 => "Cyan",
        _ => "Unknown",
    }
}

fn decode_crop_data(data: &[u8], tags: &mut Vec<Tag>) {
    // CropData: Format=int32u
    // 0=CropLeft, 1=CropTop, 2=CropRight, 3=CropBottom
    // 4=CropOutputWidthInches(double), 6=CropOutputHeightInches(double)
    // 8=CropScaledResolution(double), 10=CropSourceResolution(double)
    // 12=CropOutputResolution(double), 14=CropOutputScale(double)
    // 16=CropOutputWidth, 17=CropOutputHeight, 18=CropOutputPixels
    if data.len() >= 4 {
        tags.push(mk("CropLeft", &ru32(data, 0).to_string()));
    }
    if data.len() >= 8 {
        tags.push(mk("CropTop", &ru32(data, 4).to_string()));
    }
    if data.len() >= 12 {
        tags.push(mk("CropRight", &ru32(data, 8).to_string()));
    }
    if data.len() >= 16 {
        tags.push(mk("CropBottom", &ru32(data, 12).to_string()));
    }
    if data.len() >= 24 {
        tags.push(mk("CropOutputWidthInches", &format!("{}", rf64(data, 16))));
    }
    if data.len() >= 32 {
        tags.push(mk("CropOutputHeightInches", &format!("{}", rf64(data, 24))));
    }
    if data.len() >= 40 {
        tags.push(mk("CropScaledResolution", &format!("{}", rf64(data, 32))));
    }
    if data.len() >= 48 {
        tags.push(mk("CropSourceResolution", &format!("{}", rf64(data, 40))));
    }
    if data.len() >= 56 {
        tags.push(mk("CropOutputResolution", &format!("{}", rf64(data, 48))));
    }
    if data.len() >= 64 {
        tags.push(mk("CropOutputScale", &format!("{}", rf64(data, 56))));
    }
    if data.len() >= 68 {
        tags.push(mk("CropOutputWidth", &ru32(data, 64).to_string()));
    }
    if data.len() >= 72 {
        tags.push(mk("CropOutputHeight", &ru32(data, 68).to_string()));
    }
    if data.len() >= 76 {
        tags.push(mk("CropOutputPixels", &ru32(data, 72).to_string()));
    }
}

fn decode_exposure(data: &[u8], tags: &mut Vec<Tag>) {
    // Exposure: 0=ExposureAdj(double), 8=ExposureAdj2(double), 16=ActiveD-Lighting(int16u)
    if data.len() >= 8 {
        tags.push(mk("ExposureAdj", &format!("{}", rf64(data, 0))));
    }
    if data.len() >= 16 {
        let v = rf64(data, 8);
        tags.push(mk("ExposureAdj2", &format!("{:.4}", v)));
    }
}

fn decode_dlighting_hs(data: &[u8], tags: &mut Vec<Tag>) {
    // Format=int32u: 0=D-LightingHSAdjustment, 1=D-LightingHSColorBoost
    if data.len() >= 4 {
        tags.push(mk("D-LightingHSAdjustment", &ru32(data, 0).to_string()));
    }
    if data.len() >= 8 {
        tags.push(mk("D-LightingHSColorBoost", &ru32(data, 4).to_string()));
    }
}

fn decode_dlighting_hq(data: &[u8], tags: &mut Vec<Tag>) {
    // Format=int32u: 0=D-LightingHQShadow, 1=D-LightingHQHighlight, 2=D-LightingHQColorBoost
    if data.len() >= 4 {
        tags.push(mk("D-LightingHQShadow", &ru32(data, 0).to_string()));
    }
    if data.len() >= 8 {
        tags.push(mk("D-LightingHQHighlight", &ru32(data, 4).to_string()));
    }
    if data.len() >= 12 {
        tags.push(mk("D-LightingHQColorBoost", &ru32(data, 8).to_string()));
    }
}

fn decode_color_boost(data: &[u8], tags: &mut Vec<Tag>) {
    // Format=int8u: 0=ColorBoostType, 1=ColorBoostLevel(int32u)
    if !data.is_empty() {
        let t = match data[0] {
            0 => "Nature",
            1 => "People",
            _ => "Unknown",
        };
        tags.push(mk("ColorBoostType", t));
    }
    if data.len() >= 5 {
        tags.push(mk("ColorBoostLevel", &ru32(data, 1).to_string()));
    }
}

fn decode_noise_reduction(data: &[u8], tags: &mut Vec<Tag>) {
    // 0x04=EdgeNoiseReduction, 0x05=ColorMoireReductionMode, 0x09=Intensity(int32u),
    // 0x0d=Sharpness(int32u), 0x11=Method(int16u)
    if data.len() > 4 {
        tags.push(mk("EdgeNoiseReduction", off_on(data[4])));
    }
    if data.len() > 5 {
        let m = match data[5] {
            0 => "Off",
            1 => "Low",
            2 => "Medium",
            3 => "High",
            _ => "",
        };
        if !m.is_empty() {
            tags.push(mk("ColorMoireReductionMode", m));
        }
    }
    if data.len() >= 13 {
        tags.push(mk("NoiseReductionIntensity", &ru32(data, 9).to_string()));
    }
    if data.len() >= 17 {
        tags.push(mk("NoiseReductionSharpness", &ru32(data, 13).to_string()));
    }
    if data.len() >= 19 {
        let m = match ru16(data, 17) {
            0 => "Faster",
            1 => "Better Quality",
            2 => "Better Quality 2013",
            _ => "",
        };
        if !m.is_empty() {
            tags.push(mk("NoiseReductionMethod", m));
        }
    }
}

fn decode_brightness(data: &[u8], tags: &mut Vec<Tag>) {
    // 0=BrightnessAdj(double, *50), 8=EnhanceDarkTones
    if data.len() >= 8 {
        let v = rf64(data, 0) * 50.0;
        tags.push(mk("BrightnessAdj", &format!("{}", v)));
    }
    if data.len() > 8 {
        tags.push(mk("EnhanceDarkTones", off_on(data[8])));
    }
}

fn decode_photo_effects(data: &[u8], tags: &mut Vec<Tag>) {
    // 0=PhotoEffectsType, 4=Red(int16s), 6=Green(int16s), 8=Blue(int16s)
    if !data.is_empty() {
        let t = match data[0] {
            0 => "None",
            1 => "B&W",
            2 => "Sepia",
            3 => "Tinted",
            _ => "",
        };
        if !t.is_empty() {
            tags.push(mk("PhotoEffectsType", t));
        }
    }
    if data.len() >= 6 {
        let v = i16::from_le_bytes([data[4], data[5]]);
        tags.push(mk("PhotoEffectsRed", &v.to_string()));
    }
    if data.len() >= 8 {
        let v = i16::from_le_bytes([data[6], data[7]]);
        tags.push(mk("PhotoEffectsGreen", &v.to_string()));
    }
    if data.len() >= 10 {
        let v = i16::from_le_bytes([data[8], data[9]]);
        tags.push(mk("PhotoEffectsBlue", &v.to_string()));
    }
}

fn decode_wb_adj(data: &[u8], tags: &mut Vec<Tag>) {
    // 0x00=WBAdjRedBalance(double), 0x08=WBAdjBlueBalance(double), 0x10=WBAdjMode
    if data.len() >= 8 {
        tags.push(mk("WBAdjRedBalance", &format!("{}", rf64(data, 0))));
    }
    if data.len() >= 16 {
        tags.push(mk("WBAdjBlueBalance", &format!("{}", rf64(data, 8))));
    }
    if data.len() > 16 {
        let m = match data[16] {
            1 => "Use Gray Point",
            2 => "Recorded Value",
            3 => "Use Temperature",
            4 => "Calculate Automatically",
            5 => "Auto2",
            6 => "Underwater",
            7 => "Auto1",
            _ => "",
        };
        if !m.is_empty() {
            tags.push(mk("WBAdjMode", m));
        }
    }
    if data.len() >= 22 {
        let v = ru16(data, 20);
        tags.push(mk("WBAdjLighting", &format!("0x{:x}", v)));
    }
    if data.len() >= 26 {
        let v = ru16(data, 24);
        tags.push(mk("WBAdjTemperature", &v.to_string()));
    }
    if data.len() >= 41 {
        let v = ri32(data, 37);
        tags.push(mk("WBAdjTint", &v.to_string()));
    }
}
