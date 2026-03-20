//! TIFF file format reader.
//!
//! TIFF files are essentially a raw IFD structure, which is the same as EXIF.
//! Many RAW formats (CR2, NEF, DNG, ARW, ORF) are TIFF-based.
//! Also handles BigTIFF (magic 0x2B) and Panasonic RW2 (magic 0x55).

use crate::error::{Error, Result};
use crate::metadata::ExifReader;
use crate::tag::{Tag, TagGroup, TagId};
use crate::value::Value;

/// Extract all metadata tags from a TIFF file.
pub fn read_tiff(data: &[u8]) -> Result<Vec<Tag>> {
    if data.len() < 8 {
        return Err(Error::InvalidData("file too small for TIFF".into()));
    }

    let is_le = data[0] == b'I' && data[1] == b'I';
    let is_be = data[0] == b'M' && data[1] == b'M';

    if !is_le && !is_be {
        return Err(Error::InvalidData("not a TIFF file".into()));
    }

    let magic = if is_le {
        u16::from_le_bytes([data[2], data[3]])
    } else {
        u16::from_be_bytes([data[2], data[3]])
    };

    let mut tags = match magic {
        // Standard TIFF
        42 => ExifReader::read(data)?,
        // BigTIFF (magic 43) - IFD offset is 8 bytes at offset 8
        43 => {
            // BigTIFF has a different IFD structure:
            // - IFD entry count is 8 bytes (we only use lower 4)
            // - Each IFD entry is 20 bytes: tag(2) type(2) count(8) offset(8)
            // - Value fits inline if count * type_size <= 8
            // Parse IFD offset from BigTIFF header at bytes 8-15
            if data.len() >= 16 {
                let ifd_offset = if is_le {
                    u64::from_le_bytes([
                        data[8], data[9], data[10], data[11],
                        data[12], data[13], data[14], data[15],
                    ])
                } else {
                    u64::from_be_bytes([
                        data[8], data[9], data[10], data[11],
                        data[12], data[13], data[14], data[15],
                    ])
                } as usize;
                read_bigtiff_ifd(data, ifd_offset, is_le)?
            } else {
                vec![]
            }
        }
        // Panasonic RW2 (magic 0x55)
        0x55 => {
            read_rw2(data, is_le)?
        }
        _ => return Err(Error::InvalidData(format!("unknown TIFF magic: 0x{:04X}", magic))),
    };

    // Process GeoTiff keys if GeoTiffDirectory tag is present
    process_geotiff(&mut tags);

    Ok(tags)
}

/// Process GeoTiff directory (tag 0x87AF) and extract semantic GeoKey tags.
///
/// GeoTiff stores geographic metadata in three special TIFF tags:
///   0x87AF GeoTiffDirectory  - array of uint16: header + key entries
///   0x87B0 GeoTiffDoubleParams - array of float64 referenced by keys
///   0x87B1 GeoTiffAsciiParams  - ASCII string referenced by keys
///
/// Each GeoKey entry is 4 uint16 values: [keyId, location, count, offset]
/// - location=0: value is stored in the offset field (short integer)
/// - location=0x87AF: value is in GeoTiffDirectory (shorts) at given offset
/// - location=0x87B0: value is in GeoTiffDoubleParams (doubles) at given offset
/// - location=0x87B1: value is in GeoTiffAsciiParams (string) at given offset+count
fn process_geotiff(tags: &mut Vec<Tag>) {
    // Extract the raw GeoTiff data
    let dir_vals: Vec<u16> = {
        let tag = tags.iter().find(|t| t.name == "GeoTiffDirectory");
        match tag {
            Some(t) => extract_u16_list(&t.raw_value),
            None => return, // No GeoTiff data
        }
    };

    let double_vals: Vec<f64> = {
        let tag = tags.iter().find(|t| t.name == "GeoTiffDoubleParams");
        match tag {
            Some(t) => extract_f64_list(&t.raw_value),
            None => vec![],
        }
    };

    let ascii_val: String = {
        let tag = tags.iter().find(|t| t.name == "GeoTiffAsciiParams");
        match tag {
            Some(t) => match &t.raw_value {
                Value::String(s) => s.clone(),
                _ => String::new(),
            },
            None => String::new(),
        }
    };

    // Parse GeoTiff header: [version, revision, minorRev, numEntries]
    if dir_vals.len() < 4 {
        return;
    }
    let version = dir_vals[0];
    let revision = dir_vals[1];
    let minor_rev = dir_vals[2];
    let num_entries = dir_vals[3] as usize;

    if dir_vals.len() < 4 + num_entries * 4 {
        return;
    }

    let mut geo_tags: Vec<Tag> = Vec::new();

    // Add GeoTiffVersion (synthetic tag, not a real GeoKey)
    let version_str = format!("{}.{}.{}", version, revision, minor_rev);
    geo_tags.push(make_geotiff_tag(1, "GeoTiffVersion", "GeoTiff Version",
        Value::String(version_str.clone()), version_str));

    // Process each GeoKey entry
    for i in 0..num_entries {
        let base = 4 + i * 4;
        let key_id  = dir_vals[base];
        let location = dir_vals[base + 1];
        let count   = dir_vals[base + 2] as usize;
        let offset  = dir_vals[base + 3] as usize;

        // Get the GeoKey name and any print conversion
        let (name, description) = geotiff_key_name(key_id);
        if name.is_empty() {
            continue; // Unknown key, skip
        }

        let (raw_val, print_val) = match location {
            0 => {
                // Value is stored directly in offset field (short integer)
                let v = offset as u16;
                let raw = Value::U16(v);
                let print = geotiff_print_conv(key_id, v as i64)
                    .unwrap_or_else(|| v.to_string());
                (raw, print)
            }
            0x87B0 => {
                // Value(s) from GeoTiffDoubleParams
                if offset + count > double_vals.len() {
                    continue;
                }
                if count == 1 {
                    let v = double_vals[offset];
                    let s = format_g15(v);
                    (Value::F64(v), s)
                } else {
                    let vals: Vec<Value> = double_vals[offset..offset + count]
                        .iter().map(|&v| Value::F64(v)).collect();
                    let s = double_vals[offset..offset + count]
                        .iter().map(|&v| format_g15(v)).collect::<Vec<_>>().join(" ");
                    (Value::List(vals), s)
                }
            }
            0x87B1 => {
                // Value from GeoTiffAsciiParams string
                // offset = start index, count = length (including trailing '|' or '\0')
                let start = offset;
                let end = (start + count).min(ascii_val.len());
                let mut s = ascii_val[start..end].to_string();
                // Remove trailing '|' or '\0' terminator
                if s.ends_with('|') || s.ends_with('\0') {
                    s.pop();
                }
                (Value::String(s.clone()), s)
            }
            0x87AF => {
                // Value from GeoTiffDirectory itself (short array)
                if offset + count > dir_vals.len() {
                    continue;
                }
                if count == 1 {
                    let v = dir_vals[offset];
                    let raw = Value::U16(v);
                    let print = geotiff_print_conv(key_id, v as i64)
                        .unwrap_or_else(|| v.to_string());
                    (raw, print)
                } else {
                    let vals: Vec<Value> = dir_vals[offset..offset + count]
                        .iter().map(|&v| Value::U16(v)).collect();
                    let s = vals.iter().map(|v| v.to_display_string()).collect::<Vec<_>>().join(" ");
                    (Value::List(vals), s)
                }
            }
            _ => continue, // Unknown location
        };

        geo_tags.push(make_geotiff_tag(key_id, name, description, raw_val, print_val));
    }

    // Remove raw GeoTiff block tags (they are replaced by semantic GeoKey tags)
    tags.retain(|t| {
        t.name != "GeoTiffDirectory"
            && t.name != "GeoTiffDoubleParams"
            && t.name != "GeoTiffAsciiParams"
    });

    tags.extend(geo_tags);
}

/// Create a GeoTiff semantic tag.
fn make_geotiff_tag(key_id: u16, name: &str, description: &str, raw_val: Value, print_val: String) -> Tag {
    Tag {
        id: TagId::Numeric(key_id),
        name: name.to_string(),
        description: description.to_string(),
        group: TagGroup {
            family0: "EXIF".to_string(),
            family1: "IFD0".to_string(),
            family2: "Location".to_string(),
        },
        raw_value: raw_val,
        print_value: print_val,
        priority: 0,
    }
}

/// Format a double value like Perl's %.15g (15 significant digits).
fn format_g15(v: f64) -> String {
    crate::value::format_g15(v)
}

/// Extract a list of u16 values from a Value (handles U16, List of U16).
fn extract_u16_list(value: &Value) -> Vec<u16> {
    match value {
        Value::U16(v) => vec![*v],
        Value::List(items) => items.iter().filter_map(|item| {
            match item {
                Value::U16(v) => Some(*v),
                Value::U8(v) => Some(*v as u16),
                _ => None,
            }
        }).collect(),
        _ => vec![],
    }
}

/// Extract a list of f64 values from a Value.
fn extract_f64_list(value: &Value) -> Vec<f64> {
    match value {
        Value::F64(v) => vec![*v],
        Value::F32(v) => vec![*v as f64],
        Value::List(items) => items.iter().filter_map(|item| {
            match item {
                Value::F64(v) => Some(*v),
                Value::F32(v) => Some(*v as f64),
                _ => None,
            }
        }).collect(),
        _ => vec![],
    }
}

/// Map a GeoKey ID to (name, description).
/// Based on GeoTiff.pm key table.
fn geotiff_key_name(key_id: u16) -> (&'static str, &'static str) {
    match key_id {
        1024 => ("GTModelType", "GT Model Type"),
        1025 => ("GTRasterType", "GT Raster Type"),
        1026 => ("GTCitation", "GT Citation"),
        2048 => ("GeographicType", "Geographic Type"),
        2049 => ("GeogCitation", "Geog Citation"),
        2050 => ("GeogGeodeticDatum", "Geog Geodetic Datum"),
        2051 => ("GeogPrimeMeridian", "Geog Prime Meridian"),
        2052 => ("GeogLinearUnits", "Geog Linear Units"),
        2053 => ("GeogLinearUnitSize", "Geog Linear Unit Size"),
        2054 => ("GeogAngularUnits", "Geog Angular Units"),
        2055 => ("GeogAngularUnitSize", "Geog Angular Unit Size"),
        2056 => ("GeogEllipsoid", "Geog Ellipsoid"),
        2057 => ("GeogSemiMajorAxis", "Geog Semi Major Axis"),
        2058 => ("GeogSemiMinorAxis", "Geog Semi Minor Axis"),
        2059 => ("GeogInvFlattening", "Geog Inv Flattening"),
        2060 => ("GeogAzimuthUnits", "Geog Azimuth Units"),
        2061 => ("GeogPrimeMeridianLong", "Geog Prime Meridian Long"),
        2062 => ("GeogToWGS84", "Geog To WGS84"),
        3072 => ("ProjectedCSType", "Projected CS Type"),
        3073 => ("PCSCitation", "PCS Citation"),
        3074 => ("Projection", "Projection"),
        3075 => ("ProjCoordTrans", "Proj Coord Trans"),
        3076 => ("ProjLinearUnits", "Proj Linear Units"),
        3077 => ("ProjLinearUnitSize", "Proj Linear Unit Size"),
        3078 => ("ProjStdParallel1", "Proj Std Parallel 1"),
        3079 => ("ProjStdParallel2", "Proj Std Parallel 2"),
        3080 => ("ProjNatOriginLong", "Proj Nat Origin Long"),
        3081 => ("ProjNatOriginLat", "Proj Nat Origin Lat"),
        3082 => ("ProjFalseEasting", "Proj False Easting"),
        3083 => ("ProjFalseNorthing", "Proj False Northing"),
        3084 => ("ProjFalseOriginLong", "Proj False Origin Long"),
        3085 => ("ProjFalseOriginLat", "Proj False Origin Lat"),
        3086 => ("ProjFalseOriginEasting", "Proj False Origin Easting"),
        3087 => ("ProjFalseOriginNorthing", "Proj False Origin Northing"),
        3088 => ("ProjCenterLong", "Proj Center Long"),
        3089 => ("ProjCenterLat", "Proj Center Lat"),
        3090 => ("ProjCenterEasting", "Proj Center Easting"),
        3091 => ("ProjCenterNorthing", "Proj Center Northing"),
        3092 => ("ProjScaleAtNatOrigin", "Proj Scale At Nat Origin"),
        3093 => ("ProjScaleAtCenter", "Proj Scale At Center"),
        3094 => ("ProjAzimuthAngle", "Proj Azimuth Angle"),
        3095 => ("ProjStraightVertPoleLong", "Proj Straight Vert Pole Long"),
        4096 => ("VerticalCSType", "Vertical CS Type"),
        4097 => ("VerticalCitation", "Vertical Citation"),
        4098 => ("VerticalDatum", "Vertical Datum"),
        4099 => ("VerticalUnits", "Vertical Units"),
        _ => ("", ""),
    }
}

/// Apply print conversion for GeoTiff keys.
/// Uses the generated print_conv table keyed by ("GeoTiff", key_id).
/// Falls back to inline tables for keys not in the generated table.
fn geotiff_print_conv(key_id: u16, value: i64) -> Option<String> {
    // Try generated table first (covers GTModelType, GTRasterType, GeogPrimeMeridian, etc.)
    if let Some(s) = crate::tags::print_conv_generated::print_conv("GeoTiff", key_id, value) {
        return Some(s.to_string());
    }

    // Keys with large tables not included in generated file:
    // GeographicType (2048), GeogGeodeticDatum (2050), ProjectedCSType (3072), Projection (3074)
    match key_id {
        // GeographicType (epsg_gcs codes + User Defined)
        2048 => geotiff_epsg_gcs(value),
        // GeogGeodeticDatum (epsg_datum codes + User Defined)
        2050 => geotiff_epsg_datum(value),
        // ProjectedCSType (epsg_pcs codes + User Defined)
        3072 => geotiff_epsg_pcs(value),
        // Projection (epsg_proj codes + User Defined)
        3074 => geotiff_epsg_proj(value),
        _ => None,
    }
}

/// GeographicType print conversion (EPSG GCS codes).
/// Contains the most common values; 32767 = User Defined.
fn geotiff_epsg_gcs(value: i64) -> Option<String> {
    let s = match value {
        4001 => "Airy 1830", 4002 => "Airy Modified 1849",
        4003 => "Australian National Spheroid", 4004 => "Bessel 1841",
        4005 => "Bessel Modified", 4006 => "Bessel Namibia",
        4007 => "Clarke 1858", 4008 => "Clarke 1866",
        4009 => "Clarke 1866 Michigan", 4010 => "Clarke 1880 Benoit",
        4011 => "Clarke 1880 IGN", 4012 => "Clarke 1880 RGS",
        4013 => "Clarke 1880 Arc", 4014 => "Clarke 1880 SGA 1922",
        4015 => "Everest 1830 1937 Adjustment", 4016 => "Everest 1830 1967 Definition",
        4017 => "Everest 1830 1975 Definition", 4018 => "Everest 1830 Modified",
        4019 => "GRS 1980", 4020 => "Helmert 1906",
        4021 => "Indonesian National Spheroid", 4022 => "International 1924",
        4023 => "International 1967", 4024 => "Krassowsky 1940",
        4025 => "NWL9D", 4026 => "NWL10D", 4027 => "Plessis 1817",
        4028 => "Struve 1860", 4029 => "War Office", 4030 => "WGS84",
        4031 => "GEM10C", 4032 => "OSU86F", 4033 => "OSU91A",
        4034 => "Clarke 1880", 4035 => "Sphere",
        4120 => "Greek", 4121 => "GGRS87", 4123 => "KKJ", 4124 => "RT90",
        4133 => "EST92", 4815 => "Greek Athens",
        4201 => "Adindan", 4202 => "AGD66", 4203 => "AGD84",
        4204 => "Ain el Abd", 4205 => "Afgooye", 4206 => "Agadez",
        4267 => "NAD27", 4269 => "NAD83", 4277 => "OSGB 1936",
        4278 => "OSGB70", 4279 => "OS SN 1980",
        4283 => "GDA94", 4289 => "Amersfoort",
        4291 => "SAD69", 4292 => "Sapper Hill 1943",
        4293 => "Schwarzeck", 4297 => "Moznet",
        4298 => "Indian 1954", 4300 => "TM65",
        4301 => "Tokyo", 4302 => "Trinidad 1903",
        4303 => "TC 1948", 4304 => "Voirol 1875",
        4306 => "Bern 1938", 4307 => "Nord Sahara 1959",
        4308 => "Stockholm 1938", 4309 => "Yacare",
        4310 => "Yoff", 4311 => "Zanderij",
        4312 => "MGI", 4313 => "Belge 1972",
        4314 => "DHDN", 4315 => "Conakry 1905",
        4317 => "Dealul Piscului 1970", 4318 => "NGN",
        4319 => "KUDAMS", 4322 => "WGS 72",
        4324 => "WGS 72BE", 4326 => "WGS 84",
        32767 => "User Defined",
        _ => return None,
    };
    Some(s.to_string())
}

/// GeogGeodeticDatum print conversion (EPSG datum codes).
fn geotiff_epsg_datum(value: i64) -> Option<String> {
    let s = match value {
        6001 => "Airy 1830", 6002 => "Airy Modified 1849",
        6003 => "Australian National Spheroid", 6004 => "Bessel 1841",
        6005 => "Bessel Modified", 6006 => "Bessel Namibia",
        6007 => "Clarke 1858", 6008 => "Clarke 1866",
        6009 => "Clarke 1866 Michigan", 6010 => "Clarke 1880 Benoit",
        6011 => "Clarke 1880 IGN", 6012 => "Clarke 1880 RGS",
        6013 => "Clarke 1880 Arc", 6014 => "Clarke 1880 SGA 1922",
        6015 => "Everest 1830 1937 Adjustment", 6016 => "Everest 1830 1967 Definition",
        6017 => "Everest 1830 1975 Definition", 6018 => "Everest 1830 Modified",
        6019 => "GRS 1980", 6020 => "Helmert 1906",
        6021 => "Indonesian National Spheroid", 6022 => "International 1924",
        6023 => "International 1967", 6024 => "Krassowsky 1960",
        6025 => "NWL9D", 6026 => "NWL10D", 6027 => "Plessis 1817",
        6028 => "Struve 1860", 6029 => "War Office", 6030 => "WGS84",
        6031 => "GEM10C", 6032 => "OSU86F", 6033 => "OSU91A",
        6034 => "Clarke 1880", 6035 => "Sphere",
        6201 => "Adindan", 6202 => "AGD66", 6203 => "AGD84",
        6204 => "Ain el Abd", 6205 => "Afgooye", 6206 => "Agadez",
        6267 => "NAD27", 6269 => "NAD83", 6277 => "OSGB 1936",
        6278 => "OSGB70", 6279 => "OS SN 1980",
        6283 => "GDA94", 6289 => "Amersfoort",
        6291 => "SAD69", 6301 => "Tokyo",
        6314 => "DHDN", 6322 => "WGS 72",
        6324 => "WGS 72BE", 6326 => "WGS 84",
        32767 => "User Defined",
        _ => return None,
    };
    Some(s.to_string())
}

/// ProjectedCSType print conversion (EPSG PCS codes).
fn geotiff_epsg_pcs(value: i64) -> Option<String> {
    // WGS84 UTM zones 1N-60N (32601-32660)
    if value >= 32601 && value <= 32660 {
        return Some(format!("WGS84 UTM zone {}N", value - 32600));
    }
    // WGS84 UTM zones 1S-60S (32701-32760)
    if value >= 32701 && value <= 32760 {
        return Some(format!("WGS84 UTM zone {}S", value - 32700));
    }
    let s = match value {
        20137 => "Adindan UTM zone 37N", 20138 => "Adindan UTM zone 38N",
        20248 => "AGD66 AMG zone 48", 20249 => "AGD66 AMG zone 49",
        20250 => "AGD66 AMG zone 50", 20251 => "AGD66 AMG zone 51",
        20252 => "AGD66 AMG zone 52", 20253 => "AGD66 AMG zone 53",
        20254 => "AGD66 AMG zone 54", 20255 => "AGD66 AMG zone 55",
        20256 => "AGD66 AMG zone 56", 20257 => "AGD66 AMG zone 57",
        20258 => "AGD66 AMG zone 58",
        26701 => "NAD27 UTM zone 1N", 26702 => "NAD27 UTM zone 2N",
        26703 => "NAD27 UTM zone 3N", 26704 => "NAD27 UTM zone 4N",
        26705 => "NAD27 UTM zone 5N", 26706 => "NAD27 UTM zone 6N",
        26707 => "NAD27 UTM zone 7N", 26708 => "NAD27 UTM zone 8N",
        26709 => "NAD27 UTM zone 9N", 26710 => "NAD27 UTM zone 10N",
        26711 => "NAD27 UTM zone 11N", 26712 => "NAD27 UTM zone 12N",
        26713 => "NAD27 UTM zone 13N", 26714 => "NAD27 UTM zone 14N",
        26715 => "NAD27 UTM zone 15N", 26716 => "NAD27 UTM zone 16N",
        26717 => "NAD27 UTM zone 17N", 26718 => "NAD27 UTM zone 18N",
        26719 => "NAD27 UTM zone 19N", 26720 => "NAD27 UTM zone 20N",
        26721 => "NAD27 UTM zone 21N", 26722 => "NAD27 UTM zone 22N",
        26729 => "NAD27 Alabama East", 26730 => "NAD27 Alabama West",
        26903 => "NAD83 UTM zone 3N", 26904 => "NAD83 UTM zone 4N",
        26905 => "NAD83 UTM zone 5N", 26906 => "NAD83 UTM zone 6N",
        26907 => "NAD83 UTM zone 7N", 26908 => "NAD83 UTM zone 8N",
        26909 => "NAD83 UTM zone 9N", 26910 => "NAD83 UTM zone 10N",
        26911 => "NAD83 UTM zone 11N", 26912 => "NAD83 UTM zone 12N",
        26913 => "NAD83 UTM zone 13N", 26914 => "NAD83 UTM zone 14N",
        26915 => "NAD83 UTM zone 15N", 26916 => "NAD83 UTM zone 16N",
        26917 => "NAD83 UTM zone 17N", 26918 => "NAD83 UTM zone 18N",
        26919 => "NAD83 UTM zone 19N", 26920 => "NAD83 UTM zone 20N",
        26921 => "NAD83 UTM zone 21N", 26922 => "NAD83 UTM zone 22N",
        26923 => "NAD83 UTM zone 23N",
        32767 => "User Defined",
        _ => return None,
    };
    Some(s.to_string())
}

/// Projection print conversion (EPSG projection codes).
fn geotiff_epsg_proj(value: i64) -> Option<String> {
    let s = match value {
        10101 => "Alabama CS27 East", 10102 => "Alabama CS27 West",
        10131 => "Alabama CS83 East", 10132 => "Alabama CS83 West",
        16001 => "UTM zone 1N", 16002 => "UTM zone 2N",
        16003 => "UTM zone 3N", 16004 => "UTM zone 4N",
        16005 => "UTM zone 5N", 16006 => "UTM zone 6N",
        16007 => "UTM zone 7N", 16008 => "UTM zone 8N",
        16009 => "UTM zone 9N", 16010 => "UTM zone 10N",
        16011 => "UTM zone 11N", 16012 => "UTM zone 12N",
        16013 => "UTM zone 13N", 16014 => "UTM zone 14N",
        16015 => "UTM zone 15N", 16016 => "UTM zone 16N",
        16017 => "UTM zone 17N", 16018 => "UTM zone 18N",
        16019 => "UTM zone 19N", 16020 => "UTM zone 20N",
        16021 => "UTM zone 21N", 16022 => "UTM zone 22N",
        16023 => "UTM zone 23N", 16024 => "UTM zone 24N",
        16025 => "UTM zone 25N", 16026 => "UTM zone 26N",
        16027 => "UTM zone 27N", 16028 => "UTM zone 28N",
        16029 => "UTM zone 29N", 16030 => "UTM zone 30N",
        16031 => "UTM zone 31N", 16032 => "UTM zone 32N",
        16033 => "UTM zone 33N", 16034 => "UTM zone 34N",
        16035 => "UTM zone 35N", 16036 => "UTM zone 36N",
        16037 => "UTM zone 37N", 16038 => "UTM zone 38N",
        16039 => "UTM zone 39N", 16040 => "UTM zone 40N",
        16041 => "UTM zone 41N", 16042 => "UTM zone 42N",
        16043 => "UTM zone 43N", 16044 => "UTM zone 44N",
        16045 => "UTM zone 45N", 16046 => "UTM zone 46N",
        16047 => "UTM zone 47N", 16048 => "UTM zone 48N",
        16049 => "UTM zone 49N", 16050 => "UTM zone 50N",
        16051 => "UTM zone 51N", 16052 => "UTM zone 52N",
        16053 => "UTM zone 53N", 16054 => "UTM zone 54N",
        16055 => "UTM zone 55N", 16056 => "UTM zone 56N",
        16057 => "UTM zone 57N", 16058 => "UTM zone 58N",
        16059 => "UTM zone 59N", 16060 => "UTM zone 60N",
        16101 => "UTM zone 1S", 16102 => "UTM zone 2S",
        16103 => "UTM zone 3S", 16104 => "UTM zone 4S",
        16105 => "UTM zone 5S", 16106 => "UTM zone 6S",
        16107 => "UTM zone 7S", 16108 => "UTM zone 8S",
        16109 => "UTM zone 9S", 16110 => "UTM zone 10S",
        16111 => "UTM zone 11S", 16112 => "UTM zone 12S",
        16113 => "UTM zone 13S", 16114 => "UTM zone 14S",
        16115 => "UTM zone 15S", 16116 => "UTM zone 16S",
        16117 => "UTM zone 17S", 16118 => "UTM zone 18S",
        16119 => "UTM zone 19S", 16120 => "UTM zone 20S",
        16121 => "UTM zone 21S", 16122 => "UTM zone 22S",
        16123 => "UTM zone 23S", 16124 => "UTM zone 24S",
        16125 => "UTM zone 25S", 16126 => "UTM zone 26S",
        16127 => "UTM zone 27S", 16128 => "UTM zone 28S",
        16129 => "UTM zone 29S", 16130 => "UTM zone 30S",
        16131 => "UTM zone 31S", 16132 => "UTM zone 32S",
        16133 => "UTM zone 33S", 16134 => "UTM zone 34S",
        16135 => "UTM zone 35S", 16136 => "UTM zone 36S",
        16137 => "UTM zone 37S", 16138 => "UTM zone 38S",
        16139 => "UTM zone 39S", 16140 => "UTM zone 40S",
        16141 => "UTM zone 41S", 16142 => "UTM zone 42S",
        16143 => "UTM zone 43S", 16144 => "UTM zone 44S",
        16145 => "UTM zone 45S", 16146 => "UTM zone 46S",
        16147 => "UTM zone 47S", 16148 => "UTM zone 48S",
        16149 => "UTM zone 49S", 16150 => "UTM zone 50S",
        16151 => "UTM zone 51S", 16152 => "UTM zone 52S",
        16153 => "UTM zone 53S", 16154 => "UTM zone 54S",
        16155 => "UTM zone 55S", 16156 => "UTM zone 56S",
        16157 => "UTM zone 57S", 16158 => "UTM zone 58S",
        16159 => "UTM zone 59S", 16160 => "UTM zone 60S",
        32767 => "User Defined",
        _ => return None,
    };
    Some(s.to_string())
}

/// Read a BigTIFF IFD and return tags.
/// BigTIFF IFD entry format (20 bytes):
///   tag(2) type(2) count(8) value_or_offset(8)
/// Value fits inline if count * type_size <= 8.
fn read_bigtiff_ifd(data: &[u8], ifd_offset: usize, is_le: bool) -> Result<Vec<Tag>> {
    use crate::tags::exif as exif_tags;

    if ifd_offset + 8 > data.len() {
        return Ok(vec![]);
    }

    // BigTIFF: entry count is 8 bytes
    let entry_count = btf_read_u64(data, ifd_offset, is_le) as usize;

    let entries_start = ifd_offset + 8;
    let entry_size = 20usize; // tag(2) + type(2) + count(8) + offset(8)
    let max_entries = (data.len().saturating_sub(entries_start)) / entry_size;
    let entry_count = entry_count.min(max_entries).min(1000);

    let mut tags = Vec::new();

    for i in 0..entry_count {
        let eoff = entries_start + i * entry_size;
        if eoff + entry_size > data.len() { break; }

        let tag = btf_read_u16(data, eoff, is_le);
        let dtype = btf_read_u16(data, eoff+2, is_le);
        let count = btf_read_u64(data, eoff+4, is_le);
        let raw_offset_bytes = &data[eoff+12..eoff+20];

        let elem_size: usize = match dtype {
            1 | 2 | 6 | 7 => 1,
            3 | 8 => 2,
            4 | 9 | 11 | 13 => 4,
            5 | 10 | 12 => 8,
            _ => continue,
        };
        let total_size = elem_size.saturating_mul(count as usize);
        if total_size == 0 { continue; }

        // Get value data (inline if fits in 8 bytes, otherwise at offset)
        let value_slice: Vec<u8> = if total_size <= 8 {
            raw_offset_bytes[..total_size.min(8)].to_vec()
        } else {
            let offset = btf_read_u64(raw_offset_bytes, 0, is_le) as usize;
            if offset + total_size > data.len() { continue; }
            data[offset..offset + total_size].to_vec()
        };

        let value = match bigtiff_parse_value(&value_slice, dtype, count as usize, is_le) {
            Some(v) => v,
            None => continue,
        };

        let (name, description) = {
            match exif_tags::lookup("IFD0", tag) {
                Some(i) => (i.name.to_string(), i.description.to_string()),
                None => match exif_tags::lookup_generated(tag) {
                    Some((n, d)) => (n.to_string(), d.to_string()),
                    None => (format!("Tag0x{:04X}", tag), format!("Unknown 0x{:04X}", tag)),
                }
            }
        };

        let print_value = exif_tags::print_conv("IFD0", tag, &value)
            .or_else(|| value.as_u64().and_then(|v|
                crate::tags::print_conv_generated::print_conv_by_name(&name, v as i64))
                .map(|s| s.to_string()))
            .unwrap_or_else(|| value.to_display_string());

        tags.push(Tag {
            id: TagId::Numeric(tag),
            name,
            description,
            group: TagGroup {
                family0: "EXIF".into(),
                family1: "IFD0".into(),
                family2: "Image".into(),
            },
            raw_value: value,
            print_value,
            priority: 0,
        });
    }

    Ok(tags)
}

fn bigtiff_parse_value(data: &[u8], dtype: u16, count: usize, is_le: bool) -> Option<Value> {
    match dtype {
        1 => {
            if data.is_empty() { return None; }
            if count == 1 { Some(Value::U8(data[0])) }
            else { Some(Value::List(data.iter().map(|&b| Value::U8(b)).collect())) }
        }
        2 => Some(Value::String(String::from_utf8_lossy(data).trim_end_matches('\0').to_string())),
        3 => {
            if count == 1 { Some(Value::U16(btf_read_u16(data, 0, is_le))) }
            else { Some(Value::List((0..count).map(|i| Value::U16(btf_read_u16(data, i*2, is_le))).collect())) }
        }
        4 | 13 => {
            if count == 1 { Some(Value::U32(btf_read_u32(data, 0, is_le))) }
            else { Some(Value::List((0..count).map(|i| Value::U32(btf_read_u32(data, i*4, is_le))).collect())) }
        }
        5 => {
            if count == 1 {
                Some(Value::URational(btf_read_u32(data, 0, is_le), btf_read_u32(data, 4, is_le)))
            } else {
                Some(Value::List((0..count).map(|i| {
                    Value::URational(btf_read_u32(data, i*8, is_le), btf_read_u32(data, i*8+4, is_le))
                }).collect()))
            }
        }
        7 => Some(Value::Undefined(data.to_vec())),
        _ => None,
    }
}

fn btf_read_u16(d: &[u8], off: usize, is_le: bool) -> u16 {
    if off + 2 > d.len() { return 0; }
    if is_le { u16::from_le_bytes([d[off], d[off+1]]) }
    else { u16::from_be_bytes([d[off], d[off+1]]) }
}

fn btf_read_u32(d: &[u8], off: usize, is_le: bool) -> u32 {
    if off + 4 > d.len() { return 0; }
    if is_le { u32::from_le_bytes([d[off], d[off+1], d[off+2], d[off+3]]) }
    else { u32::from_be_bytes([d[off], d[off+1], d[off+2], d[off+3]]) }
}

fn btf_read_u64(d: &[u8], off: usize, is_le: bool) -> u64 {
    if off + 8 > d.len() { return 0; }
    if is_le { u64::from_le_bytes([d[off], d[off+1], d[off+2], d[off+3], d[off+4], d[off+5], d[off+6], d[off+7]]) }
    else { u64::from_be_bytes([d[off], d[off+1], d[off+2], d[off+3], d[off+4], d[off+5], d[off+6], d[off+7]]) }
}
