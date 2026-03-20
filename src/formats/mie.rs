//! MIE (Meta Information Encapsulation) file format reader.
//!
//! MIE is a format designed by Phil Harvey for flexible metadata storage.
//! It uses nested groups with typed data elements.
//! Mirrors ExifTool's MIE.pm.

use crate::error::{Error, Result};
use crate::tag::{Tag, TagGroup, TagId};
use crate::value::Value;

pub fn read_mie(data: &[u8]) -> Result<Vec<Tag>> {
    if data.len() < 4 || data[0] != b'~' {
        return Err(Error::InvalidData("not a MIE file".into()));
    }

    let mut tags = Vec::new();
    let mut pos = 0;
    // The top-level is the file itself, containing a "0MIE" directory
    parse_mie_group(data, &mut pos, "MIE-Top", &mut tags)?;
    Ok(tags)
}

fn parse_mie_group(data: &[u8], pos: &mut usize, group_name: &str, tags: &mut Vec<Tag>) -> Result<()> {
    while *pos + 4 <= data.len() {
        let sync = data[*pos];
        if sync != b'~' {
            break;
        }
        let format = data[*pos + 1];
        let tag_len = data[*pos + 2] as usize;
        let mut val_len = data[*pos + 3] as usize;
        *pos += 4;

        // Read tag name
        if tag_len > 0 {
            if *pos + tag_len > data.len() {
                break;
            }
        }
        let tag_name = if tag_len > 0 {
            let name = String::from_utf8_lossy(&data[*pos..*pos + tag_len]).to_string();
            *pos += tag_len;
            name
        } else {
            String::new()
        };

        // Multi-byte value length
        if val_len > 252 {
            let n = 1usize << (256 - val_len);
            if *pos + n > data.len() {
                break;
            }
            val_len = match n {
                1 => data[*pos] as usize,
                2 => u16::from_be_bytes([data[*pos], data[*pos + 1]]) as usize,
                4 => u32::from_be_bytes([data[*pos], data[*pos + 1], data[*pos + 2], data[*pos + 3]]) as usize,
                _ => break,
            };
            *pos += n;
        }

        // Group terminator (empty tag name)
        if tag_name.is_empty() {
            *pos += val_len; // skip terminator data
            break;
        }

        if *pos + val_len > data.len() {
            break;
        }

        let format_type = format & 0xfb; // strip compression bit
        let _is_compressed = format & 0x04 != 0;

        // MIE directory (format 0x10 or 0x18)
        if format_type == 0x10 || format_type == 0x18 {
            // Subdirectory - determine child group name
            let child_group = resolve_subdir_group(group_name, &tag_name);

            if val_len > 0 {
                // Embedded data subdirectory (EXIF, XMP, IPTC, ICC, etc.)
                let value_data = &data[*pos..*pos + val_len];
                match tag_name.as_str() {
                    "EXIF" => {
                        if let Ok(exif_tags) = crate::metadata::ExifReader::read(value_data) {
                            tags.extend(exif_tags);
                        }
                    }
                    "XMP" => {
                        if let Ok(xmp_tags) = crate::metadata::XmpReader::read(value_data) {
                            tags.extend(xmp_tags);
                        }
                    }
                    "IPTC" => {
                        if let Ok(iptc_tags) = crate::metadata::IptcReader::read(value_data) {
                            tags.extend(iptc_tags);
                        }
                    }
                    "ICCProfile" => {
                        if let Ok(icc_tags) = crate::formats::icc::read_icc(value_data) {
                            tags.extend(icc_tags);
                        }
                    }
                    _ => {
                        let mut sub_pos = 0;
                        let _ = parse_mie_group(value_data, &mut sub_pos, &child_group, tags);
                    }
                }
                *pos += val_len;
            } else {
                // Inline subdirectory — elements follow in-stream until group terminator
                let _ = parse_mie_group(data, pos, &child_group, tags);
            }
        } else if format_type == 0x80 {
            // Free space — skip
            *pos += val_len;
        } else {
            // Data element
            let value_data = &data[*pos..*pos + val_len];
            *pos += val_len;

            // Strip units from tag name: "Tag(unit)" -> "Tag"
            let clean_tag = if let Some(paren) = tag_name.find('(') {
                &tag_name[..paren]
            } else {
                &tag_name
            };

            // Resolve tag name based on group and MIE tag name
            let (resolved_name, family2) = resolve_tag_name(group_name, clean_tag);

            // Parse value based on format
            let value = parse_mie_value(format_type, value_data);

            tags.push(Tag {
                id: TagId::Text(resolved_name.clone()),
                name: resolved_name.clone(),
                description: resolved_name.clone(),
                group: TagGroup {
                    family0: "MIE".into(),
                    family1: group_name.into(),
                    family2: family2.into(),
                },
                print_value: value.to_display_string(),
                raw_value: value,
                priority: 0,
            });
        }
    }
    Ok(())
}

fn parse_mie_value(format_type: u8, data: &[u8]) -> Value {
    match format_type {
        0x20 | 0x28 => {
            // ASCII string or UTF-8
            Value::String(String::from_utf8_lossy(data).trim_end_matches('\0').to_string())
        }
        0x30 | 0x38 => {
            // String list (null-separated)
            let s = String::from_utf8_lossy(data).trim_end_matches('\0').to_string();
            Value::String(s.replace('\0', ", "))
        }
        0x40 => {
            // int8u
            if data.len() == 1 {
                Value::U8(data[0])
            } else {
                Value::Binary(data.to_vec())
            }
        }
        0x41 => {
            // int16u
            if data.len() >= 2 {
                let mut vals = Vec::new();
                let mut i = 0;
                while i + 2 <= data.len() {
                    vals.push(u16::from_be_bytes([data[i], data[i + 1]]));
                    i += 2;
                }
                if vals.len() == 1 {
                    Value::U16(vals[0])
                } else {
                    Value::String(vals.iter().map(|v| v.to_string()).collect::<Vec<_>>().join(" "))
                }
            } else {
                Value::Binary(data.to_vec())
            }
        }
        0x42 => {
            // int32u
            if data.len() >= 4 {
                Value::U32(u32::from_be_bytes([data[0], data[1], data[2], data[3]]))
            } else {
                Value::Binary(data.to_vec())
            }
        }
        0x48 => {
            // int8s
            if data.len() == 1 {
                Value::I16(data[0] as i8 as i16)
            } else {
                Value::Binary(data.to_vec())
            }
        }
        0x49 => {
            // int16s
            if data.len() >= 2 {
                Value::I16(i16::from_be_bytes([data[0], data[1]]))
            } else {
                Value::Binary(data.to_vec())
            }
        }
        0x52 | 0x53 => {
            // rational32u or rational64u
            if format_type == 0x53 && data.len() >= 8 {
                let num = u32::from_be_bytes([data[0], data[1], data[2], data[3]]);
                let den = u32::from_be_bytes([data[4], data[5], data[6], data[7]]);
                Value::URational(num, den)
            } else if format_type == 0x52 && data.len() >= 4 {
                let num = u16::from_be_bytes([data[0], data[1]]) as u32;
                let den = u16::from_be_bytes([data[2], data[3]]) as u32;
                Value::URational(num, den)
            } else {
                Value::Binary(data.to_vec())
            }
        }
        0x5a | 0x5b => {
            // rational32s or rational64s
            if format_type == 0x5b && data.len() >= 8 {
                let num = i32::from_be_bytes([data[0], data[1], data[2], data[3]]);
                let den = i32::from_be_bytes([data[4], data[5], data[6], data[7]]);
                Value::IRational(num, den)
            } else {
                Value::Binary(data.to_vec())
            }
        }
        0x72 => {
            // float
            if data.len() >= 4 {
                let f = f32::from_be_bytes([data[0], data[1], data[2], data[3]]);
                Value::String(format!("{}", f))
            } else {
                Value::Binary(data.to_vec())
            }
        }
        0x73 => {
            // double
            if data.len() >= 8 {
                let f = f64::from_be_bytes([data[0], data[1], data[2], data[3], data[4], data[5], data[6], data[7]]);
                Value::String(format!("{}", f))
            } else {
                Value::Binary(data.to_vec())
            }
        }
        _ => {
            // undef / binary
            if data.is_empty() {
                Value::String(String::new())
            } else {
                Value::Binary(data.to_vec())
            }
        }
    }
}

fn resolve_subdir_group(parent: &str, tag_name: &str) -> String {
    match (parent, tag_name) {
        ("MIE-Top", "0MIE") => "MIE-Main".into(),
        ("MIE-Main", "Meta") => "MIE-Meta".into(),
        ("MIE-Meta", "Audio") => "MIE-Audio".into(),
        ("MIE-Meta", "Camera") => "MIE-Camera".into(),
        ("MIE-Meta", "Document") => "MIE-Doc".into(),
        ("MIE-Meta", "Geo") => "MIE-Geo".into(),
        ("MIE-Meta", "Image") => "MIE-Image".into(),
        ("MIE-Meta", "MakerNotes") => "MIE-MakerNotes".into(),
        ("MIE-Meta", "Preview") => "MIE-Preview".into(),
        ("MIE-Meta", "Thumbnail") => "MIE-Thumbnail".into(),
        ("MIE-Meta", "Video") => "MIE-Video".into(),
        ("MIE-Camera", "Flash") => "MIE-Flash".into(),
        ("MIE-Camera", "Lens") => "MIE-Lens".into(),
        ("MIE-Camera", "Orientation") => "MIE-Orient".into(),
        ("MIE-Geo", "GPS") => "MIE-GPS".into(),
        ("MIE-Geo", "UTM") => "MIE-UTM".into(),
        ("MIE-Lens", "Extender") => "MIE-Extender".into(),
        ("MIE-MakerNotes", "Canon") => "MIE-Canon".into(),
        _ => format!("MIE-{}", tag_name),
    }
}

/// Resolve MIE internal tag names to ExifTool-compatible names.
fn resolve_tag_name(group: &str, tag: &str) -> (String, String) {
    match group {
        "MIE-Main" => {
            let name = match tag {
                "0Type" => "SubfileType",
                "0Vers" => "MIEVersion",
                "1Directory" => "SubfileDirectory",
                "1Name" => "SubfileName",
                "2MIME" => "SubfileMIMEType",
                "data" => "SubfileData",
                "rsrc" => "SubfileResource",
                "zmd5" => "MD5Digest",
                "zmie" => "TrailerSignature",
                _ => tag,
            };
            (name.into(), "Other".into())
        }
        "MIE-Doc" => {
            let name = match tag {
                "Author" => "Author",
                "Comment" => "Comment",
                "Contributors" => "Contributors",
                "Copyright" => "Copyright",
                "CreateDate" => "CreateDate",
                "EMail" => "Email",
                "Keywords" => "Keywords",
                "ModifyDate" => "ModifyDate",
                "OriginalDate" => "DateTimeOriginal",
                "Phone" => "PhoneNumber",
                "References" => "References",
                "Software" => "Software",
                "Title" => "Title",
                "URL" => "URL",
                _ => tag,
            };
            (name.into(), "Document".into())
        }
        "MIE-Geo" => {
            let name = match tag {
                "Address" => "Address",
                "City" => "City",
                "Country" => "Country",
                "PostalCode" => "PostalCode",
                "State" => "State",
                _ => tag,
            };
            (name.into(), "Location".into())
        }
        "MIE-GPS" => {
            let name = match tag {
                "Altitude" => "GPSAltitude",
                "Bearing" => "GPSDestBearing",
                "Datum" => "GPSMapDatum",
                "Differential" => "GPSDifferential",
                "Distance" => "GPSDestDistance",
                "Heading" => "GPSTrack",
                "Latitude" => "GPSLatitude",
                "Longitude" => "GPSLongitude",
                "MeasureMode" => "GPSMeasureMode",
                "Satellites" => "GPSSatellites",
                "Speed" => "GPSSpeed",
                "DateTime" => "GPSDateTime",
                _ => tag,
            };
            (name.into(), "Location".into())
        }
        "MIE-Image" => {
            let name = match tag {
                "0Type" => "FullSizeImageType",
                "1Name" => "FullSizeImageName",
                "BitDepth" => "BitDepth",
                "ColorSpace" => "ColorSpace",
                "Components" => "ComponentsConfiguration",
                "Compression" => "CompressionRatio",
                "ImageSize" => "ImageSize",
                "Resolution" => "Resolution",
                "data" => "FullSizeImage",
                _ => tag,
            };
            (name.into(), "Image".into())
        }
        "MIE-Preview" => {
            let name = match tag {
                "0Type" => "PreviewImageType",
                "1Name" => "PreviewImageName",
                "ImageSize" => "PreviewImageSize",
                "data" => "PreviewImage",
                _ => tag,
            };
            (name.into(), "Image".into())
        }
        "MIE-Thumbnail" => {
            let name = match tag {
                "0Type" => "ThumbnailImageType",
                "1Name" => "ThumbnailImageName",
                "ImageSize" => "ThumbnailImageSize",
                "data" => "ThumbnailImage",
                _ => tag,
            };
            (name.into(), "Image".into())
        }
        "MIE-Camera" => {
            let name = match tag {
                "Brightness" => "Brightness",
                "ColorTemperature" => "ColorTemperature",
                "Contrast" => "Contrast",
                "DigitalZoom" => "DigitalZoom",
                "ExposureComp" => "ExposureCompensation",
                "ExposureMode" => "ExposureMode",
                "ExposureTime" => "ExposureTime",
                "FirmwareVersion" => "FirmwareVersion",
                "FocusMode" => "FocusMode",
                "ISO" => "ISO",
                "ISOSetting" => "ISOSetting",
                "ImageNumber" => "ImageNumber",
                "ImageQuality" => "ImageQuality",
                "ImageStabilization" => "ImageStabilization",
                "Make" => "Make",
                "MeasuredEV" => "MeasuredEV",
                "Model" => "Model",
                "OwnerName" => "OwnerName",
                "Saturation" => "Saturation",
                "SerialNumber" => "SerialNumber",
                "Sharpness" => "Sharpness",
                "ShootingMode" => "ShootingMode",
                _ => tag,
            };
            (name.into(), "Camera".into())
        }
        "MIE-Lens" => {
            let name = match tag {
                "FNumber" => "FNumber",
                "FocalLength" => "FocalLength",
                "FocusDistance" => "FocusDistance",
                "Make" => "LensMake",
                "MaxAperture" => "MaxAperture",
                "MaxApertureAtMaxFocal" => "MaxApertureAtMaxFocal",
                "MaxFocalLength" => "MaxFocalLength",
                "MinAperture" => "MinAperture",
                "MinFocalLength" => "MinFocalLength",
                "Model" => "LensModel",
                "OpticalZoom" => "OpticalZoom",
                "SerialNumber" => "LensSerialNumber",
                _ => tag,
            };
            (name.into(), "Camera".into())
        }
        "MIE-Flash" => {
            let name = match tag {
                "ExposureComp" => "FlashExposureComp",
                "Fired" => "FlashFired",
                "GuideNumber" => "FlashGuideNumber",
                "Make" => "FlashMake",
                "Mode" => "FlashMode",
                "Model" => "FlashModel",
                "SerialNumber" => "FlashSerialNumber",
                "Type" => "FlashType",
                _ => tag,
            };
            (name.into(), "Camera".into())
        }
        "MIE-Orient" => {
            let name = match tag {
                "Rotation" => "Rotation",
                "Azimuth" => "Azimuth",
                "Declination" => "Declination",
                "Elevation" => "Elevation",
                "RightAscension" => "RightAscension",
                _ => tag,
            };
            (name.into(), "Camera".into())
        }
        "MIE-Audio" => {
            let name = match tag {
                "0Type" => "RelatedAudioFileType",
                "1Name" => "RelatedAudioFileName",
                "SampleBits" => "SampleBits",
                "Channels" => "Channels",
                "Compression" => "AudioCompression",
                "Duration" => "Duration",
                "SampleRate" => "SampleRate",
                "data" => "RelatedAudioFile",
                _ => tag,
            };
            (name.into(), "Audio".into())
        }
        "MIE-Video" => {
            let name = match tag {
                "0Type" => "RelatedVideoFileType",
                "1Name" => "RelatedVideoFileName",
                "Codec" => "Codec",
                "Duration" => "Duration",
                "data" => "RelatedVideoFile",
                _ => tag,
            };
            (name.into(), "Video".into())
        }
        _ => (tag.into(), "Other".into()),
    }
}
