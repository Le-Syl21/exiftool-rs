//! Composite (derived/calculated) tags.
//!
//! These tags are computed from other tags, not stored in the file.
//! Mirrors ExifTool's Composite tags.

use crate::tag::{Tag, TagGroup, TagId};
use crate::value::Value;

/// Generate composite tags from existing tags.
pub fn compute_composite_tags(tags: &[Tag]) -> Vec<Tag> {
    let mut composite = Vec::new();

    // GPSPosition: combine GPSLatitude/Ref + GPSLongitude/Ref
    if let Some(pos) = compute_gps_position(tags) {
        composite.push(pos);
    }

    // GPSAltitude: combine GPSAltitude + GPSAltitudeRef
    if let Some(alt) = compute_gps_altitude(tags) {
        composite.push(alt);
    }

    // ShutterSpeed: from ExposureTime
    if let Some(ss) = compute_shutter_speed(tags) {
        composite.push(ss);
    }

    // Aperture: from FNumber
    if let Some(ap) = compute_aperture(tags) {
        composite.push(ap);
    }

    // ShutterSpeed from ShutterSpeedValue (APEX) if no ExposureTime
    if find_tag(tags, "ShutterSpeed").is_none() && find_tag(tags, "ExposureTime").is_none() {
        if let Some(ssv) = find_tag_f64(tags, "ShutterSpeedValue") {
            let speed = 2.0_f64.powf(-ssv);
            let print = if speed >= 1.0 {
                format!("{:.0} s", speed)
            } else if speed > 0.0 {
                format!("1/{:.0} s", 1.0 / speed)
            } else { "0".to_string() };
            composite.push(mk_composite("ShutterSpeed", "Shutter Speed", Value::String(print)));
        }
    }

    // ImageSize: Width x Height
    if let Some(sz) = compute_image_size(tags) {
        composite.push(sz);
    }

    // Megapixels (needs ImageSize composite)
    {
        let mut all: Vec<Tag> = tags.to_vec();
        all.extend(composite.iter().cloned());
        if let Some(mp) = compute_megapixels(&all) {
            composite.push(mp);
        }
    }

    // LightValue
    // LightValue (needs ShutterSpeed composite)
    {
        let mut all: Vec<Tag> = tags.to_vec();
        all.extend(composite.iter().cloned());
        if let Some(lv) = compute_light_value(&all) {
            composite.push(lv);
        }
    }

    // SubSecDateTimeOriginal
    if let Some(t) = make_subsec_date(tags, "DateTimeOriginal", "SubSecTimeOriginal", "OffsetTimeOriginal", "SubSecDateTimeOriginal") {
        composite.push(t);
    }
    // SubSecCreateDate
    if let Some(t) = make_subsec_date(tags, "CreateDate", "SubSecTimeDigitized", "OffsetTimeDigitized", "SubSecCreateDate") {
        composite.push(t);
    }
    // SubSecModifyDate
    if let Some(t) = make_subsec_date(tags, "ModifyDate", "SubSecTime", "OffsetTime", "SubSecModifyDate") {
        composite.push(t);
    }

    // Geolocation: reverse geocode from GPS coordinates
    if let Some(geo_tags) = compute_geolocation(tags) {
        composite.extend(geo_tags);
    }

    // ScaleFactor35efl + FocalLength35efl + Lens35efl
    if let Some(sf_tags) = compute_35efl(tags) {
        composite.extend(sf_tags);
    } else if find_tag(tags, "FocalLength").is_some() {
        // Fallback: FocalLength35efl = FocalLength when no scale factor available
        // (Perl does: ValueConv => ($val[0] || 0) * ($val[1] || 1))
        let fl = find_tag_f64(tags, "FocalLength").unwrap_or(0.0);
        composite.push(mk_composite("FocalLength35efl", "Focal Length (35mm equiv)",
            Value::String(format!("{:.1} mm", fl))));
    }

    // RedBalance + BlueBalance
    if let Some(wb_tags) = compute_wb_balance(tags) {
        composite.extend(wb_tags);
    }

    // DOF (Depth of Field) — needs CircleOfConfusion from 35efl composites
    {
        let mut all_tags: Vec<&Tag> = tags.iter().collect();
        let comp_refs: Vec<&Tag> = composite.iter().collect();
        all_tags.extend(comp_refs);
        let all_slice: Vec<Tag> = all_tags.into_iter().cloned().collect();
        if let Some(dof_tags) = compute_dof(&all_slice) {
            composite.extend(dof_tags);
        }
        // HyperfocalDistance (needs CircleOfConfusion from composites)
        if let Some(hd) = compute_hyperfocal(&all_slice) {
            composite.push(hd);
        }
    }

    // IPTC DateTimeCreated (from IPTC:DateCreated + IPTC:TimeCreated)
    // Only combine when the source DateCreated tag is from IPTC (not RIFF, XMP, etc.)
    if find_tag(tags, "DateTimeCreated").is_none() {
        if let (Some(date_tag), Some(time)) = (find_tag_in_group(tags, "DateCreated", "IPTC"), find_tag_value(tags, "TimeCreated")) {
            let date = date_tag.print_value.clone();
            if !date.is_empty() && !time.is_empty() {
                composite.push(mk_composite("DateTimeCreated", "Date/Time Created",
                    Value::String(format!("{} {}", date, time))));
            }
        }
    }

    // DateTimeOriginal fallback (when no EXIF DateTimeOriginal)
    // Works from any DateCreated+TimeCreated pair (IPTC, RIFF, etc.)
    if find_tag(tags, "DateTimeOriginal").is_none() {
        if let (Some(date), Some(time)) = (find_tag_value(tags, "DateCreated"), find_tag_value(tags, "TimeCreated")) {
            if !date.is_empty() && !time.is_empty() {
                composite.push(mk_composite("DateTimeOriginal", "Date/Time Original",
                    Value::String(format!("{} {}", date, time))));
            }
        }
    }
    // DateTimeOriginal from ID3:Year (when no other DateTimeOriginal)
    // Perl: ID3::Composite, only fires for ID3 group tags
    if find_tag(tags, "DateTimeOriginal").is_none() && composite.iter().all(|t| t.name != "DateTimeOriginal") {
        if let Some(year_tag) = tags.iter().find(|t| t.name == "Year" && t.group.family0 == "ID3") {
            let year = year_tag.print_value.clone();
            if !year.is_empty() {
                composite.push(mk_composite("DateTimeOriginal", "Date/Time Original",
                    Value::String(year)));
            }
        }
    }

    // GPSDateTime composite (Perl: Require GPSDateStamp + GPSTimeStamp)
    // Both tags must EXIST. Date can be empty (result: " 00:00:00Z")
    if find_tag(tags, "GPSDateStamp").is_some() && find_tag(tags, "GPSTimeStamp").is_some() {
        let date = find_tag_value(tags, "GPSDateStamp").unwrap_or_default();
        let time = find_tag_value(tags, "GPSTimeStamp").unwrap_or_default();
        if !time.is_empty() {
            composite.push(mk_composite("GPSDateTime", "GPS Date/Time",
                Value::String(format!("{} {}Z", date, time))));
        }
    }

    // DigitalCreationDateTime (IPTC composite)
    if let (Some(date), Some(time)) = (
        find_tag_value(tags, "DigitalCreationDate"),
        find_tag_value(tags, "DigitalCreationTime")
    ) {
        if !date.is_empty() && !time.is_empty() {
            composite.push(mk_composite("DigitalCreationDateTime", "Digital Creation Date/Time",
                Value::String(format!("{} {}", date, time))));
        }
    }

    // LensID fallback: use LensModel or Lens if no LensID computed by 35efl
    if !composite.iter().any(|t| t.name == "LensID") {
        let lens_val = find_tag_value(tags, "LensModel")
            .filter(|v| !v.is_empty())
            .or_else(|| find_tag_value(tags, "Lens").filter(|v| !v.is_empty() && (v.contains("mm") || v.contains("/F"))));
        if let Some(lm) = lens_val {
            // Apply PrintConv: s/ - /-/ (remove spaces around dash), etc.
            let lens_id = lm.replace(" - ", "-").replace("mmF", "mm F").replace("/F", "mm F");
            composite.push(mk_composite("LensID", "Lens ID", Value::String(lens_id)));
        }
    }

    // Nikon SerialNumber (from InternalSerialNumber) - Nikon only
    {
        let make = find_tag_value(tags, "Make").unwrap_or_default();
        if make.to_uppercase().contains("NIKON") && find_tag(tags, "SerialNumber").is_none() {
            if let Some(sn) = find_tag_value(tags, "InternalSerialNumber") {
                composite.push(mk_composite("SerialNumber", "Serial Number", Value::String(sn)));
            }
        }
    }

    // LensSpec — Nikon-only composite (handled in Nikon section below)

    // Canon-specific composites
    if let Some(canon_tags) = compute_canon_composites(tags) {
        composite.extend(canon_tags);
    }

    // Nikon-specific composites
    {
        let make = find_tag_value(tags, "Make").unwrap_or_default();
        if make.to_uppercase().contains("NIKON") {
            // AutoFocus from FocusMode (only from MakerNotes, not XMP)
            if let Some(fm_tag) = tags.iter().find(|t| t.name == "FocusMode") {
                if fm_tag.group.family0 != "XMP" {
                    if find_tag(&composite, "AutoFocus").is_none() {
                        let af = if fm_tag.print_value.contains("Manual") { "Off" } else { "On" };
                        composite.push(mk_composite("AutoFocus", "Auto Focus", Value::String(af.into())));
                    }
                }
            }
            // LensSpec from Lens+LensType
            if find_tag(tags, "LensSpec").is_none() {
                if let Some(lens) = find_tag_value(tags, "Lens") {
                    if !lens.is_empty() {
                        composite.push(mk_composite("LensSpec", "Lens Spec", Value::String(lens)));
                    }
                }
            }
        }
    }

    // Kodak DateCreated composite (from YearCreated+MonthDayCreated)
    if let (Some(year), Some(md)) = (find_tag_value(tags, "YearCreated"), find_tag_value(tags, "MonthDayCreated")) {
        if !year.is_empty() && !md.is_empty() {
            composite.push(mk_composite("DateCreated", "Date Created",
                Value::String(format!("{}:{}", year, md))));
        }
    }

    composite
}

fn find_tag<'a>(tags: &'a [Tag], name: &str) -> Option<&'a Tag> {
    let name_lower = name.to_lowercase();
    tags.iter().find(|t| t.name.to_lowercase() == name_lower)
}

fn find_tag_in_group<'a>(tags: &'a [Tag], name: &str, group: &str) -> Option<&'a Tag> {
    let name_lower = name.to_lowercase();
    let group_lower = group.to_lowercase();
    tags.iter().find(|t| {
        t.name.to_lowercase() == name_lower
            && (t.group.family0.to_lowercase() == group_lower
                || t.group.family1.to_lowercase() == group_lower)
    })
}

fn find_tag_value(tags: &[Tag], name: &str) -> Option<String> {
    find_tag(tags, name).map(|t| t.print_value.clone())
}

fn find_tag_f64(tags: &[Tag], name: &str) -> Option<f64> {
    find_tag(tags, name).and_then(|t| {
        // Try direct conversion first
        t.raw_value.as_f64().or_else(|| {
            // For lists (e.g., ISO: 0, 200), take last non-zero value
            if let Value::List(items) = &t.raw_value {
                items.iter().rev().find_map(|v| {
                    let f = v.as_f64()?;
                    if f > 0.0 { Some(f) } else { None }
                })
            } else {
                // Try parsing from print value
                t.print_value.split(',').last()
                    .and_then(|s| s.trim().parse::<f64>().ok())
                    .filter(|&v| v > 0.0)
            }
        })
    })
}

fn compute_gps_position(tags: &[Tag]) -> Option<Tag> {
    let lat_tag = find_tag(tags, "GPSLatitude")?;
    let lon_tag = find_tag(tags, "GPSLongitude")?;
    let lat_ref = find_tag_value(tags, "GPSLatitudeRef").unwrap_or_default();
    let lon_ref = find_tag_value(tags, "GPSLongitudeRef").unwrap_or_default();

    let lat = format_gps_coord(&lat_tag.raw_value, &lat_ref);
    let lon = format_gps_coord(&lon_tag.raw_value, &lon_ref);

    if lat.is_empty() || lon.is_empty() {
        return None;
    }

    Some(mk_composite(
        "GPSPosition",
        "GPS Position",
        Value::String(format!("{}, {}", lat, lon)),
    ))
}

fn format_gps_coord(value: &Value, reference: &str) -> String {
    match value {
        Value::List(items) if items.len() >= 3 => {
            let deg = match items[0].as_f64() { Some(v) => v, None => return String::new() };
            let min = items[1].as_f64().unwrap_or(0.0);
            let sec = items[2].as_f64().unwrap_or(0.0);
            let decimal = deg + min / 60.0 + sec / 3600.0;
            let sign = if reference == "S" || reference == "W" { "-" } else { "" };
            format!("{}{:.6} deg", sign, decimal)
        }
        Value::URational(n, d) if *d > 0 => {
            let decimal = *n as f64 / *d as f64;
            let sign = if reference == "S" || reference == "W" { "-" } else { "" };
            format!("{}{:.6} deg", sign, decimal)
        }
        _ => String::new(),
    }
}

fn compute_gps_altitude(tags: &[Tag]) -> Option<Tag> {
    let alt = find_tag(tags, "GPSAltitude")?;
    let alt_ref = find_tag_value(tags, "GPSAltitudeRef").unwrap_or_default();

    let meters = alt.raw_value.as_f64()?;
    let sign = if alt_ref.contains("Below") { "-" } else { "" };

    Some(mk_composite(
        "GPSAltitude",
        "GPS Altitude",
        Value::String(format!("{}{:.1} m", sign, meters)),
    ))
}

fn compute_shutter_speed(tags: &[Tag]) -> Option<Tag> {
    let et = find_tag(tags, "ExposureTime")?;
    // Already has print conversion (e.g., "1/60 s"), just use it
    Some(mk_composite(
        "ShutterSpeed",
        "Shutter Speed",
        Value::String(et.print_value.clone()),
    ))
}

/// Perl: Aperture = FNumber || ApertureValue
fn compute_aperture(tags: &[Tag]) -> Option<Tag> {
    let val = find_tag_f64(tags, "FNumber")
        .or_else(|| find_tag_f64(tags, "ApertureValue")
            .map(|av| 2.0_f64.powf(av / 2.0)))?;
    if val <= 0.0 { return None; }
    Some(mk_composite("Aperture", "Aperture",
        Value::String(format!("{:.1}", val))))
}

fn compute_image_size(tags: &[Tag]) -> Option<Tag> {
    // Perl composite ImageSize requires ImageWidth and ImageHeight (format-native dimensions).
    // ExifImageWidth/Height are "Desire" tags only used for Canon/Phase One TIFF-based RAW.
    // Do not fall back to ExifImageWidth/Height to avoid computing ImageSize for formats
    // (e.g. PDF) that have embedded EXIF but no native image dimensions.
    let w = find_tag(tags, "ImageWidth")?;
    let h = find_tag(tags, "ImageHeight")?;

    let width = w.raw_value.as_u64().or_else(|| w.print_value.parse().ok())?;
    let height = h.raw_value.as_u64().or_else(|| h.print_value.parse().ok())?;

    Some(mk_composite(
        "ImageSize",
        "Image Size",
        Value::String(format!("{}x{}", width, height)),
    ))
}

/// Perl: Require => 'ImageSize', ValueConv => 'my @d = ($val =~ /\d+/g); $d[0] * $d[1] / 1000000'
fn compute_megapixels(tags: &[Tag]) -> Option<Tag> {
    // Use ImageSize composite (already computed) like Perl does
    let sz = find_tag_value(tags, "ImageSize")
        .or_else(|| {
            let w = find_tag(tags, "ImageWidth")?;
            let h = find_tag(tags, "ImageHeight")?;
            let wv = w.raw_value.as_u64().or_else(|| w.print_value.parse().ok())?;
            let hv = h.raw_value.as_u64().or_else(|| h.print_value.parse().ok())?;
            Some(format!("{}x{}", wv, hv))
        })?;

    let nums: Vec<f64> = sz.split(|c: char| !c.is_ascii_digit())
        .filter_map(|s| s.parse().ok()).collect();
    if nums.len() < 2 { return None; }

    let mp = nums[0] * nums[1] / 1_000_000.0;
    // Perl: sprintf("%.*f", ($val >= 1 ? 1 : ($val >= 0.001 ? 3 : 6)), $val)
    let fmt = if mp >= 1.0 { format!("{:.1}", mp) }
        else if mp >= 0.001 { format!("{:.3}", mp) }
        else { format!("{:.6}", mp) };

    Some(mk_composite(
        "Megapixels",
        "Megapixels",
        Value::String(fmt),
    ))
}

/// Perl: LV = 2*log2(Aperture) - log2(ShutterSpeed) - log2(ISO/100)
/// Uses composites Aperture, ShutterSpeed, ISO (not raw FNumber/ExposureTime)
fn compute_light_value(tags: &[Tag]) -> Option<Tag> {
    // Aperture from composite or FNumber or ApertureValue
    let aperture = find_tag_f64(tags, "FNumber")
        .or_else(|| find_tag_f64(tags, "ApertureValue")
            .map(|av| 2.0_f64.powf(av / 2.0)))?;

    // ShutterSpeed from ExposureTime or ShutterSpeedValue
    let shutter = find_tag_f64(tags, "ExposureTime")
        .or_else(|| find_tag_f64(tags, "ShutterSpeedValue")
            .map(|sv| 2.0_f64.powf(-sv)))
        .or_else(|| {
            // Parse from ShutterSpeed composite print value like "1/60 s"
            find_tag_value(tags, "ShutterSpeed").and_then(|s| {
                let s = s.trim_end_matches(" s").trim();
                if s.contains('/') {
                    let parts: Vec<&str> = s.split('/').collect();
                    let n: f64 = parts[0].parse().ok()?;
                    let d: f64 = parts[1].parse().ok()?;
                    Some(n / d)
                } else { s.parse().ok() }
            })
        })?;

    let iso = find_tag_f64(tags, "ISO")?;

    if shutter <= 0.0 || iso <= 0.0 || aperture <= 0.0 {
        return None;
    }

    // LV = 2*log2(Aperture) - log2(ShutterSpeed) - log2(ISO/100)
    let lv = 2.0 * aperture.log2() - shutter.log2() - (iso / 100.0).log2();

    Some(mk_composite(
        "LightValue",
        "Light Value",
        Value::String(format!("{:.1}", lv)),
    ))
}

/// Compute 35mm equivalent focal length and scale factor.
fn compute_35efl(tags: &[Tag]) -> Option<Vec<Tag>> {
    let fl = find_tag_f64(tags, "FocalLength")?;
    if fl <= 0.0 { return None; }

    let mut result = Vec::new();

    // Compute scale factor (Perl: CalcScaleFactor35efl)
    // Sources: FocalLengthIn35mmFormat, FocalPlaneDiagonal, FocalPlaneResolution
    let scale = if let Some(fl35) = find_tag_f64(tags, "FocalLengthIn35mmFormat") {
        if fl35 > 0.0 { fl35 / fl } else { return None; }
    } else if let Some(diag) = find_tag_f64(tags, "FocalPlaneDiagonal")
        .or_else(|| find_tag_value(tags, "FocalPlaneDiagonal")
            .and_then(|s| s.split_whitespace().next()?.parse().ok()))
    {
        // Sanity check: diagonal must be reasonable (1-100mm)
        if diag > 1.0 && diag < 100.0 { 43.2666 / diag } else { return None; }
    } else if let Some(fpxs) = find_tag_f64(tags, "FocalPlaneXSize")
            .and_then(|v| if v < 100.0 { Some(v) } else { None }) // Skip raw U16 values (914 etc.)
            .or_else(|| find_tag_value(tags, "FocalPlaneXSize")
                .and_then(|s| s.split_whitespace().next()?.parse::<f64>().ok()))
    {
        // FocalPlaneXSize/YSize path (mm values)
        let fpys = find_tag_f64(tags, "FocalPlaneYSize")
            .and_then(|v| if v < 100.0 { Some(v) } else { None })
            .or_else(|| find_tag_value(tags, "FocalPlaneYSize")
                .and_then(|s| s.split_whitespace().next()?.parse::<f64>().ok()))?;
        let diag = (fpxs * fpxs + fpys * fpys).sqrt();
        if diag > 1.0 && diag < 100.0 { 43.2666 / diag } else { return None; }
    } else {
        // Compute from sensor size via FocalPlaneResolution
        let fpxr = find_tag_f64(tags, "FocalPlaneXResolution")?;
        let fpyr = find_tag_f64(tags, "FocalPlaneYResolution")?;
        // Use largest available image dimensions (full sensor)
        let img_w = find_tag_f64(tags, "RelatedImageWidth")
            .or_else(|| find_tag_f64(tags, "ExifImageWidth"))?;
        let img_h = find_tag_f64(tags, "RelatedImageHeight")
            .or_else(|| find_tag_f64(tags, "ExifImageHeight"))?;
        if fpxr <= 0.0 || fpyr <= 0.0 || img_w <= 0.0 || img_h <= 0.0 { return None; }

        let unit = find_tag_f64(tags, "FocalPlaneResolutionUnit").unwrap_or(2.0);
        let factor = match unit as u32 { 2 => 25.4, 3 => 10.0, _ => 25.4 };
        let sensor_w = img_w * factor / fpxr;
        let sensor_h = img_h * factor / fpyr;
        let sensor_diag = (sensor_w * sensor_w + sensor_h * sensor_h).sqrt();
        // Sanity check
        if sensor_diag <= 1.0 || sensor_diag >= 100.0 { return None; }
        // Aspect ratio sanity check
        let ratio = if sensor_w > sensor_h { sensor_w / sensor_h } else { sensor_h / sensor_w };
        if ratio > 3.0 { return None; }
        43.2666 / sensor_diag
    };

    let fl35_val = fl * scale;

    result.push(mk_composite("ScaleFactor35efl", "Scale Factor To 35 mm Equivalent",
        Value::String(format!("{:.1}", scale))));
    result.push(mk_composite("FocalLength35efl", "Focal Length (35mm equivalent)",
        Value::String(format!("{:.1} mm (35 mm equivalent: {:.1} mm)", fl, fl35_val))));

    // CircleOfConfusion: Perl formula = sqrt(24²+36²) / (scale * 1440)
    let coc = (24.0_f64.powi(2) + 36.0_f64.powi(2)).sqrt() / (scale * 1440.0);
    result.push(mk_composite_raw("CircleOfConfusion", "Circle of Confusion",
        Value::F64(coc), format!("{:.3} mm", coc)));

    // FOV
    let fov = 2.0 * (36.0 / (2.0 * fl35_val)).atan() * 180.0 / std::f64::consts::PI;
    result.push(mk_composite("FOV", "Field of View",
        Value::String(format!("{:.1} deg", fov))));

    // Lens + Lens35efl (Canon-specific)
    let make = find_tag_value(tags, "Make").unwrap_or_default();
    let min_fl = find_tag_f64(tags, "MinFocalLength");
    let max_fl = find_tag_f64(tags, "MaxFocalLength");
    if let (Some(min), Some(max)) = (min_fl, max_fl) {
        if min > 0.0 && max > 0.0 && max > min {
            result.push(mk_composite("Lens", "Lens",
                Value::String(format!("{:.1} - {:.1} mm", min, max))));
            // Lens35efl only for Canon
            if make.contains("Canon") {
                result.push(mk_composite("Lens35efl", "Lens (35mm equivalent)",
                    Value::String(format!("{:.1} - {:.1} mm (35 mm equivalent: {:.1} - {:.1} mm)",
                        min, max, min * scale, max * scale))));
            }
        }
    }

    // LensID
    if let Some(lt) = find_tag(tags, "LensType") {
        let lens_str = lt.print_value.clone();
        if !lens_str.is_empty() && lens_str != "0" {
            result.push(mk_composite("LensID", "Lens ID", Value::String(lens_str)));
        }
    }

    Some(result)
}

/// Build SubSec composite date.
/// Only emit when subsec or offset actually adds information.
fn make_subsec_date(tags: &[Tag], date_tag: &str, subsec_tag: &str, offset_tag: &str, output_name: &str) -> Option<Tag> {
    let dt = find_tag_value(tags, date_tag)?;
    if dt.is_empty() { return None; }

    let subsec = find_tag_value(tags, subsec_tag).unwrap_or_default();
    let offset = find_tag_value(tags, offset_tag).unwrap_or_default();

    let mut result = dt.clone();
    let mut modified = false;

    // Only add subsec if date doesn't already have subseconds (contains '.')
    if !subsec.is_empty() && !dt.contains('.') {
        result = format!("{}.{}", result, subsec.trim());
        modified = true;
    }
    // Only add offset if date doesn't already have timezone (contains '+' or '-' after time part)
    if !offset.is_empty() && !dt.contains('+') && !(dt.len() > 10 && dt[10..].contains('-')) {
        result = format!("{}{}", result, offset.trim());
        modified = true;
    }

    if !modified { return None; }

    Some(mk_composite(output_name, output_name, Value::String(result)))
}

/// Canon-specific composites.
fn compute_canon_composites(tags: &[Tag]) -> Option<Vec<Tag>> {
    // Only if this is a Canon file
    let make = find_tag_value(tags, "Make").unwrap_or_default();
    if !make.contains("Canon") { return None; }

    let mut result = Vec::new();

    // DriveMode (from ContinuousDrive)
    if let Some(cd) = find_tag_value(tags, "ContinuousDrive") {
        result.push(mk_composite("DriveMode", "Drive Mode", Value::String(cd)));
    }

    // ShootingMode (from CanonExposureMode)
    if let Some(em) = find_tag_value(tags, "CanonExposureMode") {
        result.push(mk_composite("ShootingMode", "Shooting Mode", Value::String(em)));
    }

    // Canon Lens composite from MinFocalLength+MaxFocalLength
    let min_fl = find_tag_f64(tags, "MinFocalLength");
    let max_fl = find_tag_f64(tags, "MaxFocalLength");
    if let (Some(min), Some(max)) = (min_fl, max_fl) {
        if min > 0.0 && max > 0.0 {
            let spec = format!("{:.0}-{:.0}mm", min, max);
            result.push(mk_composite("Lens", "Lens", Value::String(spec)));
        }
    }

    // FileNumber (Canon composite): DirectoryIndex + FileIndex → "DDD-FFFF"
    // Perl: sprintf("%.3d%.4d", @val), then PrintConv s/(\d+)(\d{4})/$1-$2/
    if let (Some(dir_idx), Some(file_idx)) = (
        find_tag_value(tags, "DirectoryIndex"),
        find_tag_value(tags, "FileIndex"),
    ) {
        if let (Ok(di), Ok(fi)) = (dir_idx.trim().parse::<i64>(), file_idx.trim().parse::<i64>()) {
            if di > 0 || fi > 0 {
                // Handle wrap: if FileIndex == 10000, it wraps (FileIndex=1, DirectoryIndex++)
                let (di2, fi2) = if fi == 10000 { (di + 1, 1i64) } else { (di, fi) };
                let combined = format!("{:03}{:04}", di2, fi2);
                // PrintConv: s/(\d+)(\d{4})/$1-$2/  (last 4 digits are file number)
                let len = combined.len();
                let print = if len > 4 {
                    format!("{}-{}", &combined[..len-4], &combined[len-4..])
                } else { combined.clone() };
                let t = Tag {
                    id: TagId::Text("FileNumber".into()),
                    name: "FileNumber".into(),
                    description: "File Number".into(),
                    group: TagGroup {
                        family0: "Composite".into(), family1: "Composite".into(), family2: "Image".into(),
                    },
                    raw_value: Value::String(combined),
                    print_value: print,
                    priority: 0,
                };
                result.push(t);
            }
        }
    }

    if result.is_empty() { None } else { Some(result) }
}

/// Compute white balance RGB ratios.
fn compute_wb_balance(tags: &[Tag]) -> Option<Vec<Tag>> {
    // Look for WB_RGGBLevels in Canon MakerNotes (raw array)
    // Or compute from XResolution ratio
    let mut result = Vec::new();

    // Try to find WhiteBalance RGGB values from Canon tags
    // These would come from Canon ColorData (tag 0x4001) which we decode separately
    // For now, check if we have the data from MakerNotes
    if let Some(wb) = find_tag(tags, "WB_RGGBLevels")
        .or_else(|| find_tag(tags, "WB_RGBGLevels"))
        .or_else(|| find_tag(tags, "WB_RBLevels"))
    {
        // Parse WB levels from either List or space-separated String
        let parts: Vec<f64> = match &wb.raw_value {
            Value::List(items) => items.iter().filter_map(|v| v.as_f64()).collect(),
            Value::String(s) => s.split_whitespace().filter_map(|p| p.parse().ok()).collect(),
            _ => Vec::new(),
        };
        if parts.len() >= 4 {
            let (r, g, b) = (parts[0], parts[1], parts[2]);
            // For RGGB: R/G1, B/G1; For RGBG: R/G, B/G2
            let g_div = if wb.name.contains("RGBG") { parts[3] } else { g };
            if g > 0.0 && g_div > 0.0 {
                result.push(mk_composite("RedBalance", "Red Balance",
                    Value::String(format!("{:.6}", r / g))));
                result.push(mk_composite("BlueBalance", "Blue Balance",
                    Value::String(format!("{:.6}", b / g_div))));
            }
        } else if parts.len() == 2 {
            // WB_RBLevels (Olympus): R/256, B/256
            let (r, b) = (parts[0], parts[1]);
            result.push(mk_composite("RedBalance", "Red Balance",
                Value::String(format!("{:.6}", r / 256.0))));
            result.push(mk_composite("BlueBalance", "Blue Balance",
                Value::String(format!("{:.6}", b / 256.0))));
        }
    } else if let Some(wb) = find_tag(tags, "WB_GRGBLevels") {
        // Fujifilm GRGB format: G, R, G, B
        // RedBalance = R / avg(G1, G2); BlueBalance = B / avg(G1, G2)
        let parts: Vec<f64> = match &wb.raw_value {
            Value::List(items) => items.iter().filter_map(|v| v.as_f64()).collect(),
            Value::String(s) => s.split_whitespace().filter_map(|p| p.parse().ok()).collect(),
            _ => Vec::new(),
        };
        if parts.len() >= 4 {
            let g1 = parts[0]; // G
            let r  = parts[1]; // R
            let g2 = parts[2]; // G
            let b  = parts[3]; // B
            let g_avg = (g1 + g2) / 2.0;
            if g_avg > 0.0 {
                // PrintConv: int($val * 1e6 + 0.5) * 1e-6, then Perl %s = %.15g format
                let red_bal = (r / g_avg * 1e6 + 0.5) as i64 as f64 * 1e-6;
                let blue_bal = (b / g_avg * 1e6 + 0.5) as i64 as f64 * 1e-6;
                let red_print = crate::value::format_g15(red_bal);
                let blue_print = crate::value::format_g15(blue_bal);
                result.push(mk_composite("RedBalance", "Red Balance",
                    Value::String(red_print)));
                result.push(mk_composite("BlueBalance", "Blue Balance",
                    Value::String(blue_print)));
            }
        }
    }

    if result.is_empty() { None } else { Some(result) }
}

/// Compute Depth of Field.
/// Compute DOF using exact Perl ExifTool formula from Exif.pm line 4775.
/// Require: FocalLength, Aperture (=FNumber), CircleOfConfusion
/// Desire: FocusDistance, SubjectDistance, FocusDistanceLower/Upper
fn compute_dof(tags: &[Tag]) -> Option<Vec<Tag>> {
    let f = find_tag_f64(tags, "FocalLength")?;     // mm
    let aperture = find_tag_f64(tags, "FNumber")?;
    let coc = find_tag_f64(tags, "CircleOfConfusion")
        .or_else(|| {
            find_tag_value(tags, "CircleOfConfusion")
                .and_then(|s| s.split_whitespace().next()?.parse::<f64>().ok())
        })?;

    if f <= 0.0 || coc <= 0.0 { return None; }

    // Find focus distance (meters). Try multiple sources like Perl does.
    let d = find_tag_f64(tags, "FocusDistance")
        .or_else(|| find_tag_value(tags, "FocusDistance")
            .and_then(|s| s.split_whitespace().next()?.parse().ok()))
        // Perl: $val[4] || $val[5] || $val[6] — 0 means "not available" for these
        .or_else(|| find_tag_f64(tags, "SubjectDistance").filter(|&v| v > 0.0))
        .or_else(|| find_tag_f64(tags, "ObjectDistance").filter(|&v| v > 0.0)
            .or_else(|| find_tag_value(tags, "ObjectDistance")
                .and_then(|s| s.split_whitespace().next()?.parse().ok())
                .filter(|&v: &f64| v > 0.0)))
        .or_else(|| find_tag_f64(tags, "ApproximateFocusDistance").filter(|&v| v > 0.0)
            .or_else(|| find_tag_value(tags, "ApproximateFocusDistance")
                .and_then(|s| s.split_whitespace().next()?.parse().ok())
                .filter(|&v: &f64| v > 0.0)))
        .or_else(|| {
            let upper = find_tag_f64(tags, "FocusDistanceUpper")
                .or_else(|| find_tag_value(tags, "FocusDistanceUpper")
                    .and_then(|s| s.split_whitespace().next()?.parse().ok()));
            let lower = find_tag_f64(tags, "FocusDistanceLower")
                .or_else(|| find_tag_value(tags, "FocusDistanceLower")
                    .and_then(|s| s.split_whitespace().next()?.parse().ok()));
            match (upper, lower) {
                (Some(u), Some(l)) => Some((u + l) / 2.0),
                _ => None,
            }
        });

    // Require focus distance (return None if missing)
    let d = d?;
    let d = if d == 0.0 { 1e10 } else { d }; // 0 = infinity

    // Perl formula: t = aperture * coc * (d*1000 - f) / (f * f)
    let t = aperture * coc * (d * 1000.0 - f) / (f * f);
    let near = d / (1.0 + t);
    let mut far = d / (1.0 - t);
    if far < 0.0 { far = 0.0; } // 0 means infinity

    let dof_str = if far == 0.0 {
        format!("inf ({:.2} m - inf)", near)
    } else {
        let dof = far - near;
        if dof > 0.0 && dof < 0.02 {
            format!("{:.3} m ({:.3} - {:.3} m)", dof, near, far)
        } else {
            format!("{:.2} m ({:.2} - {:.2} m)", dof, near, far)
        }
    };

    Some(vec![mk_composite("DOF", "Depth of Field", Value::String(dof_str))])
}


/// Reverse geocode GPS position using Geolocation.dat.
fn compute_geolocation(tags: &[Tag]) -> Option<Vec<Tag>> {
    use crate::geolocation::GeolocationDb;
    use std::sync::OnceLock;

    // Parse GPS coordinates
    let lat_tag = find_tag(tags, "GPSLatitude")?;
    let lon_tag = find_tag(tags, "GPSLongitude")?;
    let lat_ref = find_tag_value(tags, "GPSLatitudeRef").unwrap_or_default();
    let lon_ref = find_tag_value(tags, "GPSLongitudeRef").unwrap_or_default();

    let lat = parse_gps_decimal(&lat_tag.raw_value, &lat_ref)?;
    let lon = parse_gps_decimal(&lon_tag.raw_value, &lon_ref)?;

    // Load database (cached via OnceLock)
    static DB: OnceLock<Option<GeolocationDb>> = OnceLock::new();
    let db = DB.get_or_init(|| GeolocationDb::load_default());

    let db = db.as_ref()?;
    let city = db.find_nearest(lat, lon)?;

    let mut geo_tags = Vec::new();
    geo_tags.push(mk_composite("GPSCity", "GPS City", Value::String(city.name.clone())));
    geo_tags.push(mk_composite("GPSCountryCode", "GPS Country Code", Value::String(city.country_code.clone())));
    geo_tags.push(mk_composite("GPSCountry", "GPS Country", Value::String(city.country.clone())));
    if !city.region.is_empty() {
        geo_tags.push(mk_composite("GPSRegion", "GPS Region", Value::String(city.region.clone())));
    }
    if !city.subregion.is_empty() {
        geo_tags.push(mk_composite("GPSSubregion", "GPS Subregion", Value::String(city.subregion.clone())));
    }
    if !city.timezone.is_empty() {
        geo_tags.push(mk_composite("GPSTimezone", "GPS Timezone", Value::String(city.timezone.clone())));
    }

    Some(geo_tags)
}

fn parse_gps_decimal(value: &Value, reference: &str) -> Option<f64> {
    let decimal = match value {
        Value::List(items) if items.len() >= 3 => {
            let deg = items[0].as_f64()?;
            let min = items[1].as_f64()?;
            let sec = items[2].as_f64()?;
            deg + min / 60.0 + sec / 3600.0
        }
        Value::URational(n, d) if *d > 0 => *n as f64 / *d as f64,
        _ => return None,
    };
    let sign = if reference == "S" || reference == "W" { -1.0 } else { 1.0 };
    Some(decimal * sign)
}


/// Compute Hyperfocal Distance: H = f² / (N × c) + f
/// Requires CircleOfConfusion from composites (not hardcoded).
fn compute_hyperfocal(tags: &[Tag]) -> Option<Tag> {
    let fl = find_tag_f64(tags, "FocalLength")?;
    let fnum = find_tag_f64(tags, "FNumber")?;

    if fl <= 0.0 || fnum <= 0.0 {
        return None;
    }

    // Get CircleOfConfusion from composites
    let coc = find_tag_f64(tags, "CircleOfConfusion")
        .or_else(|| {
            find_tag_value(tags, "CircleOfConfusion")
                .and_then(|s| s.split_whitespace().next()?.parse::<f64>().ok())
        })?;

    if coc <= 0.0 { return None; }

    // Perl formula: $val[0]^2 / ($val[1] * $val[2] * 1000)
    // where val[0]=FocalLength(mm), val[1]=Aperture(f-number), val[2]=CoC(mm)
    // The /1000 converts from mm to m (result directly in m, no need to divide again)
    let h_m = (fl * fl) / (fnum * coc * 1000.0);

    Some(mk_composite(
        "HyperfocalDistance",
        "Hyperfocal Distance",
        Value::String(format!("{:.2} m", h_m)),
    ))
}


fn mk_composite_raw(name: &str, description: &str, value: Value, print_value: String) -> Tag {
    Tag {
        id: TagId::Text(name.to_string()),
        name: name.to_string(),
        description: description.to_string(),
        group: TagGroup {
            family0: "Composite".to_string(),
            family1: "Composite".to_string(),
            family2: "Other".to_string(),
        },
        raw_value: value,
        print_value,
        priority: 0,
    }
}

fn mk_composite(name: &str, description: &str, value: Value) -> Tag {
    let pv = value.to_display_string();
    Tag {
        id: TagId::Text(name.to_string()),
        name: name.to_string(),
        description: description.to_string(),
        group: TagGroup {
            family0: "Composite".to_string(),
            family1: "Composite".to_string(),
            family2: "Other".to_string(),
        },
        raw_value: value,
        print_value: pv,
        priority: 0,
    }
}
