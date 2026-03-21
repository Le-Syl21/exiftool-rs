//! Composite (derived/calculated) tags.
//!
//! These tags are computed from other tags, not stored in the file.
//! Mirrors ExifTool's Composite tags.

use crate::tag::{Tag, TagGroup, TagId};
use crate::value::Value;

/// Generate composite tags from existing tags.
pub fn compute_composite_tags(tags: &[Tag]) -> Vec<Tag> {
    let mut composite = Vec::new();

    // Canon-specific composites first — they produce ISO, WB_RGGBLevels, FlashType, etc.
    // that are needed by later composites (LightValue needs ISO, RedBalance needs WB_RGGBLevels)
    if let Some(canon_tags) = compute_canon_composites(tags) {
        composite.extend(canon_tags);
    }

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

    // PanasonicRaw: ImageWidth/ImageHeight composites from sensor borders
    // Perl PanasonicRaw::Composite: ImageWidth = SensorRightBorder - SensorLeftBorder
    //                               ImageHeight = SensorBottomBorder - SensorTopBorder
    // Only emit when these sensor border tags are present (RW2 files)
    if find_tag(tags, "SensorRightBorder").is_some() && find_tag(tags, "SensorLeftBorder").is_some() {
        if let (Some(right), Some(left)) = (
            find_tag(tags, "SensorRightBorder").and_then(|t| t.raw_value.as_u64()),
            find_tag(tags, "SensorLeftBorder").and_then(|t| t.raw_value.as_u64()),
        ) {
            composite.push(mk_composite("ImageWidth", "Image Width",
                Value::String(format!("{}", right.saturating_sub(left)))));
        }
    }
    if find_tag(tags, "SensorBottomBorder").is_some() && find_tag(tags, "SensorTopBorder").is_some() {
        if let (Some(bottom), Some(top)) = (
            find_tag(tags, "SensorBottomBorder").and_then(|t| t.raw_value.as_u64()),
            find_tag(tags, "SensorTopBorder").and_then(|t| t.raw_value.as_u64()),
        ) {
            composite.push(mk_composite("ImageHeight", "Image Height",
                Value::String(format!("{}", bottom.saturating_sub(top)))));
        }
    }

    // ImageSize: Width x Height
    {
        let mut all: Vec<Tag> = tags.to_vec();
        all.extend(composite.iter().cloned());
        if let Some(sz) = compute_image_size(&all) {
            composite.push(sz);
        }
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

    // RedBalance + BlueBalance — search in raw tags + Canon composites (which may include WB_RGGBLevels)
    {
        let mut all_for_wb: Vec<Tag> = tags.to_vec();
        all_for_wb.extend(composite.iter().cloned());
        if let Some(wb_tags) = compute_wb_balance(&all_for_wb) {
            composite.extend(wb_tags);
        }
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

    // LensID fallback: use LensModel, Lens, or LensType if no LensID computed by 35efl
    // Only create when the value looks like a real camera lens (contains "mm" or "f/")
    if !composite.iter().any(|t| t.name == "LensID") {
        let lens_val = find_tag_value(tags, "LensModel")
            .filter(|v| !v.is_empty() && (v.contains("mm") || v.to_lowercase().contains("f/")))
            .or_else(|| find_tag_value(tags, "Lens").filter(|v| !v.is_empty() && (v.contains("mm") || v.contains("/F"))))
            .or_else(|| find_tag_value(tags, "LensType").filter(|v| !v.is_empty() && v.contains("mm")));
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
            // Nikon LensID composite: construct 8-byte key from LensData fields
            // Require: LensIDNumber + MinFocalLength (to distinguish from generic LensType fallback)
            if !composite.iter().any(|t| t.name == "LensID")
                && find_tag(tags, "LensIDNumber").is_some()
                && find_tag(tags, "MinFocalLength").is_some()
            {
                if let Some(lens_id) = compute_nikon_lens_id(tags) {
                    composite.push(mk_composite("LensID", "Lens ID", Value::String(lens_id)));
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

    // Panasonic AdvancedSceneMode composite (PanasonicRaw::Composite — only for RW2 files)
    // Only fire when PanasonicRaw-specific tags exist (e.g. SensorTopBorder)
    if find_tag(tags, "AdvancedSceneMode").is_none()
        && find_tag(tags, "SensorTopBorder").is_some()
    {
        if let Some(adv) = compute_panasonic_advanced_scene_mode(tags) {
            composite.push(adv);
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
                // Try parsing from print value (strip units like "mm", "m", etc.)
                t.print_value.split(',').last()
                    .and_then(|s| {
                        let s = s.trim();
                        // Try direct parse first, then strip common suffixes
                        s.parse::<f64>().ok()
                            .or_else(|| s.trim_end_matches(" mm").trim().parse::<f64>().ok())
                            .or_else(|| s.trim_end_matches(" m").trim().parse::<f64>().ok())
                            .or_else(|| s.split_whitespace().next()?.parse::<f64>().ok())
                    })
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
    // For PanasonicRaw, ImageWidth/ImageHeight composites are computed before this runs.
    // ExifImageWidth/Height are "Desire" tags only used for Canon/Phase One TIFF-based RAW.
    // Do not fall back to ExifImageWidth/Height to avoid computing ImageSize for formats
    // (e.g. PDF) that have embedded EXIF but no native image dimensions.
    let width = find_tag(tags, "ImageWidth")
        .and_then(|t| t.raw_value.as_u64()
            .or_else(|| t.print_value.trim().parse().ok()));
    let height = find_tag(tags, "ImageHeight")
        .and_then(|t| t.raw_value.as_u64()
            .or_else(|| t.print_value.trim().parse().ok()));

    let (width, height) = (width?, height?);

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
        // FocalPlaneYResolution defaults to X if not present (e.g., Lytro: "Y same as X")
        let fpyr = find_tag_f64(tags, "FocalPlaneYResolution").unwrap_or(fpxr);
        // Use largest available image dimensions (full sensor)
        let img_w = find_tag_f64(tags, "RelatedImageWidth")
            .or_else(|| find_tag_f64(tags, "ExifImageWidth"))
            .or_else(|| find_tag_f64(tags, "ImageWidth"))?;
        let img_h = find_tag_f64(tags, "RelatedImageHeight")
            .or_else(|| find_tag_f64(tags, "ExifImageHeight"))
            .or_else(|| find_tag_f64(tags, "ImageHeight"))?;
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

    // LensID (Canon PrintLensID logic)
    if let Some(lt) = find_tag(tags, "LensType") {
        let raw_val = lt.raw_value.as_u64().map(|v| v as i64).unwrap_or_else(|| {
            lt.raw_value.to_display_string().parse::<i64>().unwrap_or(0)
        });
        let pv = lt.print_value.clone();
        // For LensType = -1 or 65535 ("n/a" / "Unknown"): "Unknown ShortFocal-LongFocalmm"
        if raw_val == -1 || raw_val == 65535 {
            let min_fl = find_tag_f64(tags, "MinFocalLength");
            let max_fl = find_tag_f64(tags, "MaxFocalLength");
            let lens_str = if let (Some(min), Some(max)) = (min_fl, max_fl) {
                if min > 0.0 && max > 0.0 && (max - min).abs() > 0.1 {
                    format!("Unknown {:.0}-{:.0}mm", min, max)
                } else if min > 0.0 {
                    format!("Unknown {:.0}mm", min)
                } else {
                    "Unknown".to_string()
                }
            } else {
                "Unknown".to_string()
            };
            result.push(mk_composite("LensID", "Lens ID", Value::String(lens_str)));
        } else if !pv.is_empty() && pv != "0" && pv != "n/a" {
            result.push(mk_composite("LensID", "Lens ID", Value::String(pv)));
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

    // NOTE: Canon Lens composite is handled by compute_lens_composite (main lens function)
    // which correctly formats it as "18.0 - 55.0 mm". Do not duplicate it here.

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

    // Canon ISO composite:
    // Perl: use CameraISO if numeric, else BaseISO * AutoISO / 100
    // Priority=0 (EXIF ISO takes precedence over Canon ISO composite)
    if find_tag(tags, "ISO").is_none() {
        let camera_iso_str = find_tag_value(tags, "CameraISO");
        let iso_val = camera_iso_str.as_deref()
            .and_then(|ci| ci.trim().parse::<f64>().ok())
            .filter(|&v| v > 0.0);
        let iso = iso_val.or_else(|| {
            let base = find_tag_f64(tags, "BaseISO")?;
            let auto = find_tag_f64(tags, "AutoISO")?;
            if base > 0.0 && auto > 0.0 {
                Some(base * auto / 100.0)
            } else { None }
        });
        if let Some(iso_v) = iso {
            result.push(mk_composite("ISO", "ISO", Value::String(format!("{:.0}", iso_v))));
        }
    }

    // Canon FlashType composite:
    // Perl: Require FlashBits; RawConv: suppress if FlashBits==0;
    //        ValueConv: FlashBits & (1<<14) ? 1 : 0
    if let Some(flash_bits_tag) = find_tag(tags, "FlashBits") {
        let fb_raw = flash_bits_tag.raw_value.as_u64()
            .or_else(|| flash_bits_tag.raw_value.as_f64().map(|v| v as u64))
            .unwrap_or(0);
        if fb_raw != 0 {
            let flash_type = if (fb_raw & (1 << 14)) != 0 { "External" } else { "Built-In Flash" };
            result.push(mk_composite("FlashType", "Flash Type", Value::String(flash_type.to_string())));
        }
    }

    // RedEyeReduction composite:
    // Perl: Require CanonFlashMode + FlashBits; suppress if FlashBits==0
    //        ValueConv: (CanonFlashMode==3 or ==4 or ==6) ? 1 : 0
    if let Some(flash_bits_tag) = find_tag(tags, "FlashBits") {
        let fb_raw = flash_bits_tag.raw_value.as_u64()
            .or_else(|| flash_bits_tag.raw_value.as_f64().map(|v| v as u64))
            .unwrap_or(0);
        if fb_raw != 0 {
            if let Some(cfm_tag) = find_tag(tags, "CanonFlashMode") {
                let cfm_raw = cfm_tag.raw_value.as_u64()
                    .or_else(|| cfm_tag.print_value.parse::<u64>().ok())
                    .unwrap_or(99);
                let red_eye = if cfm_raw == 3 || cfm_raw == 4 || cfm_raw == 6 { "On" } else { "Off" };
                result.push(mk_composite("RedEyeReduction", "Red Eye Reduction", Value::String(red_eye.to_string())));
            }
        }
    }

    // ConditionalFEC composite (Flash Exposure Compensation, only when flash fired):
    // Perl: Require FlashExposureComp + FlashBits; suppress if FlashBits==0
    //        ValueConv: FlashExposureComp; PrintConv: same as FlashExposureComp PrintConv
    if let Some(flash_bits_tag) = find_tag(tags, "FlashBits") {
        let fb_raw = flash_bits_tag.raw_value.as_u64()
            .or_else(|| flash_bits_tag.raw_value.as_f64().map(|v| v as u64))
            .unwrap_or(0);
        if fb_raw != 0 {
            if let Some(fec_tag) = find_tag(tags, "FlashExposureComp") {
                result.push(mk_composite("ConditionalFEC", "Flash Exposure Compensation",
                    Value::String(fec_tag.print_value.clone())));
            }
        }
    }

    // ShutterCurtainHack composite:
    // Perl: Desire ShutterCurtainSync + Require FlashBits; suppress if FlashBits==0
    //        ValueConv: defined(ShutterCurtainSync) ? ShutterCurtainSync : 0
    //        PrintConv: 0 => '1st-curtain sync', 1 => '2nd-curtain sync'
    if let Some(flash_bits_tag) = find_tag(tags, "FlashBits") {
        let fb_raw = flash_bits_tag.raw_value.as_u64()
            .or_else(|| flash_bits_tag.raw_value.as_f64().map(|v| v as u64))
            .unwrap_or(0);
        if fb_raw != 0 {
            let scs = find_tag(tags, "ShutterCurtainSync")
                .and_then(|t| t.raw_value.as_u64())
                .unwrap_or(0);
            let pv = if scs == 0 { "1st-curtain sync" } else { "2nd-curtain sync" };
            result.push(mk_composite("ShutterCurtainHack", "Shutter Curtain Sync",
                Value::String(pv.to_string())));
        }
    }

    // WB_RGGBLevels composite (Canon):
    // Perl: Require Canon:WhiteBalance; Desire WB_RGGBLevelsAsShot + many WB_ sets
    // ValueConv: '$val[1] ? $val[1] : $val[($val[0] || 0) + 2]'
    // This means: use WB_RGGBLevelsAsShot if present, else use the WB set for WhiteBalance+2
    if find_tag(tags, "WB_RGGBLevels").is_none() {
        if let Some(wb_tag) = find_tag(tags, "WhiteBalance") {
            let wb_val = wb_tag.raw_value.as_u64().unwrap_or(0);
            // Try WB_RGGBLevelsAsShot first
            let wb_str = if let Some(asshot) = find_tag(tags, "WB_RGGBLevelsAsShot") {
                Some(asshot.print_value.clone())
            } else {
                // Fall back to the set corresponding to WhiteBalance value
                // Perl: index maps: 0=Auto, 1=Daylight, 2=Cloudy, 3=Tungsten,
                //   4=Fluorescent, 5=Flash, 6=Custom, 8=Shade, 9=Kelvin
                let wb_tag_name = match wb_val {
                    0 => "WB_RGGBLevelsAuto",
                    1 => "WB_RGGBLevelsDaylight",
                    2 => "WB_RGGBLevelsCloudy",
                    3 => "WB_RGGBLevelsTungsten",
                    4 => "WB_RGGBLevelsFluorescent",
                    5 => "WB_RGGBLevelsFlash",
                    6 => "WB_RGGBLevelsCustom",
                    8 => "WB_RGGBLevelsShade",
                    9 => "WB_RGGBLevelsKelvin",
                    _ => "WB_RGGBLevelsAuto",
                };
                find_tag(tags, wb_tag_name).map(|t| t.print_value.clone())
            };
            if let Some(wb_levels) = wb_str {
                if !wb_levels.is_empty() {
                    result.push(mk_composite("WB_RGGBLevels", "WB RGGB Levels",
                        Value::String(wb_levels)));
                }
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

    // Panasonic: WBRedLevel, WBGreenLevel, WBBlueLevel as separate EXIF tags
    // Perl: RedBalance = $r/$g, BlueBalance = $b/$g (from Exif.pm Composite::RedBalance)
    if result.is_empty() {
        let r_tag = find_tag(tags, "WBRedLevel")
            .and_then(|t| t.raw_value.as_f64());
        let g_tag = find_tag(tags, "WBGreenLevel")
            .and_then(|t| t.raw_value.as_f64());
        let b_tag = find_tag(tags, "WBBlueLevel")
            .and_then(|t| t.raw_value.as_f64());
        if let (Some(r), Some(g), Some(b)) = (r_tag, g_tag, b_tag) {
            if g > 0.0 {
                // Perl formula: int($val * 1e6 + 0.5) * 1e-6 (from Exif.pm)
                let red_bal = (r / g * 1e6 + 0.5) as i64 as f64 * 1e-6;
                let blue_bal = (b / g * 1e6 + 0.5) as i64 as f64 * 1e-6;
                result.push(mk_composite("RedBalance", "Red Balance",
                    Value::String(crate::value::format_g15(red_bal))));
                result.push(mk_composite("BlueBalance", "Blue Balance",
                    Value::String(crate::value::format_g15(blue_bal))));
            }
        }
    }

    if result.is_empty() { None } else { Some(result) }
}

/// Panasonic AdvancedSceneMode composite.
/// Perl: Require => Model + SceneMode + AdvancedSceneType
/// Key = "SceneMode AdvancedSceneType" (raw integer values), optionally prefixed by model.
fn compute_panasonic_advanced_scene_mode(tags: &[Tag]) -> Option<Tag> {
    // Require all three
    let model = find_tag_value(tags, "Model")?;
    // Need SceneMode raw value (integer) and AdvancedSceneType raw value (integer)
    let scene_mode_raw = find_tag(tags, "SceneMode")
        .and_then(|t| t.raw_value.as_u64())
        .unwrap_or(0);
    let adv_type_raw = find_tag(tags, "AdvancedSceneType")
        .and_then(|t| t.raw_value.as_u64())
        .unwrap_or(1);

    // Check it's a Panasonic camera (Make = Panasonic or Leica)
    let make = find_tag_value(tags, "Make").unwrap_or_default();
    if !make.contains("Panasonic") && !make.contains("Leica") {
        return None;
    }

    // Perl PrintConv: first try model-specific key, then generic key
    let model_key = format!("{} {} {}", model, scene_mode_raw, adv_type_raw);
    let generic_key = format!("{} {}", scene_mode_raw, adv_type_raw);

    // Model-specific table (only DMC-TZ40 entries in Perl)
    let model_val = if model == "DMC-TZ40" {
        match generic_key.as_str() {
            "90 1" => Some("Expressive"),
            "90 2" => Some("Retro"),
            "90 3" => Some("High Key"),
            "90 4" => Some("Sepia"),
            "90 5" => Some("High Dynamic"),
            "90 6" => Some("Miniature"),
            "90 9" => Some("Low Key"),
            "90 10" => Some("Toy Effect"),
            "90 11" => Some("Dynamic Monochrome"),
            "90 12" => Some("Soft"),
            _ => None,
        }
    } else { None };

    let print_val = if let Some(v) = model_val {
        v.to_string()
    } else {
        // Generic table lookup
        match generic_key.as_str() {
            "0 1" => "Off".to_string(),
            "2 2" => "Outdoor Portrait".to_string(),
            "2 3" => "Indoor Portrait".to_string(),
            "2 4" => "Creative Portrait".to_string(),
            "3 2" => "Nature".to_string(),
            "3 3" => "Architecture".to_string(),
            "3 4" => "Creative Scenery".to_string(),
            "4 2" => "Outdoor Sports".to_string(),
            "4 3" => "Indoor Sports".to_string(),
            "4 4" => "Creative Sports".to_string(),
            "9 2" => "Flower".to_string(),
            "9 3" => "Objects".to_string(),
            "9 4" => "Creative Macro".to_string(),
            "18 1" => "High Sensitivity".to_string(),
            "20 1" => "Fireworks".to_string(),
            "21 2" => "Illuminations".to_string(),
            "21 4" => "Creative Night Scenery".to_string(),
            "26 1" => "High-speed Burst (shot 1)".to_string(),
            "27 1" => "High-speed Burst (shot 2)".to_string(),
            "29 1" => "Snow".to_string(),
            "30 1" => "Starry Sky".to_string(),
            "31 1" => "Beach".to_string(),
            "36 1" => "High-speed Burst (shot 3)".to_string(),
            "39 1" => "Aerial Photo / Underwater / Multi-aspect".to_string(),
            "45 2" => "Cinema".to_string(),
            "45 7" => "Expressive".to_string(),
            "45 8" => "Retro".to_string(),
            "45 9" => "Pure".to_string(),
            "45 10" => "Elegant".to_string(),
            "45 12" => "Monochrome".to_string(),
            "45 13" => "Dynamic Art".to_string(),
            "45 14" => "Silhouette".to_string(),
            "51 2" => "HDR Art".to_string(),
            "51 3" => "HDR B&W".to_string(),
            "59 1" => "Expressive".to_string(),
            "59 2" => "Retro".to_string(),
            "59 3" => "High Key".to_string(),
            "59 4" => "Sepia".to_string(),
            "59 5" => "High Dynamic".to_string(),
            "59 6" => "Miniature".to_string(),
            "59 9" => "Low Key".to_string(),
            "59 10" => "Toy Effect".to_string(),
            "59 11" => "Dynamic Monochrome".to_string(),
            "59 12" => "Soft".to_string(),
            "66 1" => "Impressive Art".to_string(),
            "66 2" => "Cross Process".to_string(),
            "66 3" => "Color Select".to_string(),
            "66 4" => "Star".to_string(),
            "90 3" => "Old Days".to_string(),
            "90 4" => "Sunshine".to_string(),
            "90 5" => "Bleach Bypass".to_string(),
            "90 6" => "Toy Pop".to_string(),
            "90 7" => "Fantasy".to_string(),
            "90 8" => "Monochrome".to_string(),
            "90 9" => "Rough Monochrome".to_string(),
            "90 10" => "Silky Monochrome".to_string(),
            "92 1" => "Handheld Night Shot".to_string(),
            _ => {
                // OTHER handler: lookup shooting mode name, add AdvancedSceneType modifier
                // shootingMode table (Panasonic.pm %shootingMode)
                let shooting_mode_name = panasonic_shooting_mode(scene_mode_raw);
                if let Some(name) = shooting_mode_name {
                    match adv_type_raw {
                        1 => name.to_string(),
                        5 => format!("{} (intelligent auto)", name),
                        7 => format!("{} (intelligent auto plus)", name),
                        n => format!("{} ({})", name, n),
                    }
                } else {
                    format!("Unknown ({} {})", scene_mode_raw, adv_type_raw)
                }
            }
        }
    };

    // Raw value: "Model SceneMode AdvancedSceneType" (Perl ValueConv)
    let raw_str = format!("{} {} {}", model, scene_mode_raw, adv_type_raw);
    Some(mk_composite_raw("AdvancedSceneMode", "Advanced Scene Mode",
        Value::String(raw_str), print_val))
}

/// Panasonic ShootingMode/SceneMode name lookup (from %shootingMode in Panasonic.pm)
fn panasonic_shooting_mode(val: u64) -> Option<&'static str> {
    match val {
        1 => Some("Normal"),
        2 => Some("Portrait"),
        3 => Some("Scenery"),
        4 => Some("Sports"),
        5 => Some("Night Portrait"),
        6 => Some("Program"),
        7 => Some("Aperture Priority"),
        8 => Some("Shutter Priority"),
        9 => Some("Macro"),
        10 => Some("Spot"),
        11 => Some("Manual"),
        12 => Some("Movie Preview"),
        13 => Some("Panning"),
        14 => Some("Simple"),
        15 => Some("Color Effects"),
        16 => Some("Self Portrait"),
        17 => Some("Economy"),
        18 => Some("Fireworks"),
        19 => Some("Party"),
        20 => Some("Snow"),
        21 => Some("Night Scenery"),
        22 => Some("Food"),
        23 => Some("Baby"),
        24 => Some("Soft Skin"),
        25 => Some("Candlelight"),
        26 => Some("Starry Night"),
        27 => Some("High Sensitivity"),
        28 => Some("Panorama Assist"),
        29 => Some("Underwater"),
        30 => Some("Beach"),
        31 => Some("Aerial Photo"),
        32 => Some("Sunset"),
        33 => Some("Pet"),
        34 => Some("Intelligent ISO"),
        35 => Some("Clipboard"),
        36 => Some("High Speed Continuous Shooting"),
        37 => Some("Intelligent Auto"),
        39 => Some("Multi-aspect"),
        41 => Some("Transform"),
        42 => Some("Flash Burst"),
        43 => Some("Pin Hole"),
        44 => Some("Film Grain"),
        45 => Some("My Color"),
        46 => Some("Photo Frame"),
        48 => Some("Movie"),
        51 => Some("HDR"),
        52 => Some("Peripheral Defocus"),
        55 => Some("Handheld Night Shot"),
        57 => Some("3D"),
        59 => Some("Creative Control"),
        60 => Some("Intelligent Auto Plus"),
        62 => Some("Panorama"),
        63 => Some("Glass Through"),
        64 => Some("HDR"),
        66 => Some("Digital Filter"),
        67 => Some("Clear Portrait"),
        68 => Some("Silky Skin"),
        69 => Some("Backlit Softness"),
        70 => Some("Clear in Backlight"),
        71 => Some("Relaxing Tone"),
        72 => Some("Sweet Child's Face"),
        73 => Some("Distinct Scenery"),
        74 => Some("Bright Blue Sky"),
        75 => Some("Romantic Sunset Glow"),
        76 => Some("Vivid Sunset Glow"),
        77 => Some("Glistening Water"),
        78 => Some("Clear Nightscape"),
        79 => Some("Cool Night Sky"),
        _ => None,
    }
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

/// Compute Nikon LensID from lens data tags.
/// Perl: ValueConv => 'sprintf("%.2X"." %.2X"x7, @raw)', PrintConv => \%nikonLensIDs
/// The 8 raw bytes are: LensIDNumber, LensFStops, MinFocalLength, MaxFocalLength,
/// MaxApertureAtMinFocal, MaxApertureAtMaxFocal, MCUVersion, LensType
fn compute_nikon_lens_id(tags: &[Tag]) -> Option<String> {
    // Byte 0: LensIDNumber (raw integer value)
    let lens_id_num = find_tag(tags, "LensIDNumber")
        .and_then(|t| t.raw_value.as_u64()
            .or_else(|| t.print_value.trim().parse::<u64>().ok()))? as u8;

    // Byte 1: LensFStops raw byte
    // From main Nikon tag 0x008B (undef[4]): bytes a,b,c,d → val = a*(b/c)
    // The raw byte for LensID key = byte 0 of the Undefined value (= a)
    let lens_fstops_byte = find_tag(tags, "LensFStops").and_then(|t| {
        match &t.raw_value {
            Value::Undefined(bytes) if !bytes.is_empty() => Some(bytes[0]),
            _ => {
                // Fall back: reverse from print value (val * 12)
                t.print_value.trim().parse::<f64>().ok()
                    .map(|v| (v * 12.0).round() as u8)
            }
        }
    }).unwrap_or(0);

    // Byte 2: MinFocalLength raw byte — reverse: 24 * log2(val/5)
    let min_focal_byte = find_tag(tags, "MinFocalLength").and_then(|t| {
        t.raw_value.as_f64()
            .or_else(|| t.print_value.split_whitespace().next()?.parse::<f64>().ok())
            .filter(|&v| v > 0.0)
            .map(|v| (24.0 * (v / 5.0).log2()).round() as u8)
    }).unwrap_or(0);

    // Byte 3: MaxFocalLength raw byte — reverse: 24 * log2(val/5)
    let max_focal_byte = find_tag(tags, "MaxFocalLength").and_then(|t| {
        t.raw_value.as_f64()
            .or_else(|| t.print_value.split_whitespace().next()?.parse::<f64>().ok())
            .filter(|&v| v > 0.0)
            .map(|v| (24.0 * (v / 5.0).log2()).round() as u8)
    }).unwrap_or(0);

    // Byte 4: MaxApertureAtMinFocal raw byte — reverse: 24 * log2(val)
    let max_apt_min_byte = find_tag(tags, "MaxApertureAtMinFocal").and_then(|t| {
        t.raw_value.as_f64()
            .or_else(|| t.print_value.trim().parse::<f64>().ok())
            .filter(|&v| v > 0.0)
            .map(|v| (24.0 * v.log2()).round() as u8)
    }).unwrap_or(0);

    // Byte 5: MaxApertureAtMaxFocal raw byte — reverse: 24 * log2(val)
    let max_apt_max_byte = find_tag(tags, "MaxApertureAtMaxFocal").and_then(|t| {
        t.raw_value.as_f64()
            .or_else(|| t.print_value.trim().parse::<f64>().ok())
            .filter(|&v| v > 0.0)
            .map(|v| (24.0 * v.log2()).round() as u8)
    }).unwrap_or(0);

    // Byte 6: MCUVersion (raw integer; may be wrong due to makernotes offset issue)
    let mcu_version_byte = find_tag(tags, "MCUVersion").and_then(|t| {
        t.raw_value.as_u64()
            .or_else(|| t.print_value.trim().parse::<u64>().ok())
    }).unwrap_or(0) as u8;

    // Byte 7: LensType (raw integer value, lower byte)
    let lens_type_byte = find_tag(tags, "LensType").and_then(|t| {
        t.raw_value.as_u64()
            .or_else(|| t.print_value.trim().parse::<u64>().ok())
    }).unwrap_or(0) as u8;

    let key = [lens_id_num, lens_fstops_byte, min_focal_byte, max_focal_byte,
               max_apt_min_byte, max_apt_max_byte, mcu_version_byte, lens_type_byte];

    nikon_lens_id_lookup(&key)
}

/// Look up Nikon lens name from 8-byte key.
/// First tries exact match, then partial match ignoring byte 6 (MCUVersion).
fn nikon_lens_id_lookup(key: &[u8; 8]) -> Option<String> {
    // Try exact match first
    for &(ref k, name) in NIKON_LENS_IDS {
        if k == key {
            return Some(name.to_string());
        }
    }
    // Partial match: ignore byte 6 (MCUVersion may not be stored correctly)
    // Match on bytes 0,1,2,3,4,5,7
    let mut matches: Vec<&str> = Vec::new();
    for &(ref k, name) in NIKON_LENS_IDS {
        if k[0] == key[0] && k[1] == key[1] && k[2] == key[2] && k[3] == key[3]
            && k[4] == key[4] && k[5] == key[5] && k[7] == key[7]
        {
            matches.push(name);
        }
    }
    if matches.len() == 1 {
        return Some(matches[0].to_string());
    }
    None
}

/// Nikon lens ID lookup table.
/// Keys are 8 bytes: LensIDNumber, LensFStops, MinFocalLength, MaxFocalLength,
/// MaxApertureAtMinFocal, MaxApertureAtMaxFocal, MCUVersion, LensType.
/// From Perl ExifTool Nikon.pm %nikonLensIDs.
static NIKON_LENS_IDS: &[([u8; 8], &str)] = &[
    ([0x01,0x58,0x50,0x50,0x14,0x14,0x02,0x00], "AF Nikkor 50mm f/1.8"),
    ([0x01,0x58,0x50,0x50,0x14,0x14,0x05,0x00], "AF Nikkor 50mm f/1.8"),
    ([0x02,0x42,0x44,0x5C,0x2A,0x34,0x02,0x00], "AF Zoom-Nikkor 35-70mm f/3.3-4.5"),
    ([0x02,0x42,0x44,0x5C,0x2A,0x34,0x08,0x00], "AF Zoom-Nikkor 35-70mm f/3.3-4.5"),
    ([0x03,0x48,0x5C,0x81,0x30,0x30,0x02,0x00], "AF Zoom-Nikkor 70-210mm f/4"),
    ([0x04,0x48,0x3C,0x3C,0x24,0x24,0x03,0x00], "AF Nikkor 28mm f/2.8"),
    ([0x05,0x54,0x50,0x50,0x0C,0x0C,0x04,0x00], "AF Nikkor 50mm f/1.4"),
    ([0x06,0x54,0x53,0x53,0x24,0x24,0x06,0x00], "AF Micro-Nikkor 55mm f/2.8"),
    ([0x07,0x40,0x3C,0x62,0x2C,0x34,0x03,0x00], "AF Zoom-Nikkor 28-85mm f/3.5-4.5"),
    ([0x08,0x40,0x44,0x6A,0x2C,0x34,0x04,0x00], "AF Zoom-Nikkor 35-105mm f/3.5-4.5"),
    ([0x09,0x48,0x37,0x37,0x24,0x24,0x04,0x00], "AF Nikkor 24mm f/2.8"),
    ([0x0A,0x48,0x8E,0x8E,0x24,0x24,0x03,0x00], "AF Nikkor 300mm f/2.8 IF-ED"),
    ([0x0A,0x48,0x8E,0x8E,0x24,0x24,0x05,0x00], "AF Nikkor 300mm f/2.8 IF-ED N"),
    ([0x0B,0x48,0x7C,0x7C,0x24,0x24,0x05,0x00], "AF Nikkor 180mm f/2.8 IF-ED"),
    ([0x0D,0x40,0x44,0x72,0x2C,0x34,0x07,0x00], "AF Zoom-Nikkor 35-135mm f/3.5-4.5"),
    ([0x0E,0x48,0x5C,0x81,0x30,0x30,0x05,0x00], "AF Zoom-Nikkor 70-210mm f/4"),
    ([0x0F,0x58,0x50,0x50,0x14,0x14,0x05,0x00], "AF Nikkor 50mm f/1.8 N"),
    ([0x10,0x48,0x8E,0x8E,0x30,0x30,0x08,0x00], "AF Nikkor 300mm f/4 IF-ED"),
    ([0x11,0x48,0x44,0x5C,0x24,0x24,0x08,0x00], "AF Zoom-Nikkor 35-70mm f/2.8"),
    ([0x11,0x48,0x44,0x5C,0x24,0x24,0x15,0x00], "AF Zoom-Nikkor 35-70mm f/2.8"),
    ([0x12,0x48,0x5C,0x81,0x30,0x3C,0x09,0x00], "AF Nikkor 70-210mm f/4-5.6"),
    ([0x13,0x42,0x37,0x50,0x2A,0x34,0x0B,0x00], "AF Zoom-Nikkor 24-50mm f/3.3-4.5"),
    ([0x14,0x48,0x60,0x80,0x24,0x24,0x0B,0x00], "AF Zoom-Nikkor 80-200mm f/2.8 ED"),
    ([0x15,0x4C,0x62,0x62,0x14,0x14,0x0C,0x00], "AF Nikkor 85mm f/1.8"),
    ([0x17,0x3C,0xA0,0xA0,0x30,0x30,0x0F,0x00], "Nikkor 500mm f/4 P ED IF"),
    ([0x17,0x3C,0xA0,0xA0,0x30,0x30,0x11,0x00], "Nikkor 500mm f/4 P ED IF"),
    ([0x18,0x40,0x44,0x72,0x2C,0x34,0x0E,0x00], "AF Zoom-Nikkor 35-135mm f/3.5-4.5 N"),
    ([0x1A,0x54,0x44,0x44,0x18,0x18,0x11,0x00], "AF Nikkor 35mm f/2"),
    ([0x1B,0x44,0x5E,0x8E,0x34,0x3C,0x10,0x00], "AF Zoom-Nikkor 75-300mm f/4.5-5.6"),
    ([0x1C,0x48,0x30,0x30,0x24,0x24,0x12,0x00], "AF Nikkor 20mm f/2.8"),
    ([0x1D,0x42,0x44,0x5C,0x2A,0x34,0x12,0x00], "AF Zoom-Nikkor 35-70mm f/3.3-4.5 N"),
    ([0x1E,0x54,0x56,0x56,0x24,0x24,0x13,0x00], "AF Micro-Nikkor 60mm f/2.8"),
    ([0x1F,0x54,0x6A,0x6A,0x24,0x24,0x14,0x00], "AF Micro-Nikkor 105mm f/2.8"),
    ([0x20,0x48,0x60,0x80,0x24,0x24,0x15,0x00], "AF Zoom-Nikkor 80-200mm f/2.8 ED"),
    ([0x21,0x40,0x3C,0x5C,0x2C,0x34,0x16,0x00], "AF Zoom-Nikkor 28-70mm f/3.5-4.5"),
    ([0x22,0x48,0x72,0x72,0x18,0x18,0x16,0x00], "AF DC-Nikkor 135mm f/2"),
    ([0x23,0x30,0xBE,0xCA,0x3C,0x48,0x17,0x00], "Zoom-Nikkor 1200-1700mm f/5.6-8 P ED IF"),
    ([0x24,0x48,0x60,0x80,0x24,0x24,0x1A,0x02], "AF Zoom-Nikkor 80-200mm f/2.8D ED"),
    ([0x25,0x48,0x44,0x5C,0x24,0x24,0x1B,0x02], "AF Zoom-Nikkor 35-70mm f/2.8D"),
    ([0x25,0x48,0x44,0x5C,0x24,0x24,0x3A,0x02], "AF Zoom-Nikkor 35-70mm f/2.8D"),
    ([0x25,0x48,0x44,0x5C,0x24,0x24,0x52,0x02], "AF Zoom-Nikkor 35-70mm f/2.8D"),
    ([0x26,0x40,0x3C,0x5C,0x2C,0x34,0x1C,0x02], "AF Zoom-Nikkor 28-70mm f/3.5-4.5D"),
    ([0x27,0x48,0x8E,0x8E,0x24,0x24,0x1D,0x02], "AF-I Nikkor 300mm f/2.8D IF-ED"),
    ([0x27,0x48,0x8E,0x8E,0x24,0x24,0xF1,0x02], "AF-I Nikkor 300mm f/2.8D IF-ED + TC-14E"),
    ([0x27,0x48,0x8E,0x8E,0x24,0x24,0xE1,0x02], "AF-I Nikkor 300mm f/2.8D IF-ED + TC-17E"),
    ([0x27,0x48,0x8E,0x8E,0x24,0x24,0xF2,0x02], "AF-I Nikkor 300mm f/2.8D IF-ED + TC-20E"),
    ([0x28,0x3C,0xA6,0xA6,0x30,0x30,0x1D,0x02], "AF-I Nikkor 600mm f/4D IF-ED"),
    ([0x28,0x3C,0xA6,0xA6,0x30,0x30,0xF1,0x02], "AF-I Nikkor 600mm f/4D IF-ED + TC-14E"),
    ([0x28,0x3C,0xA6,0xA6,0x30,0x30,0xE1,0x02], "AF-I Nikkor 600mm f/4D IF-ED + TC-17E"),
    ([0x28,0x3C,0xA6,0xA6,0x30,0x30,0xF2,0x02], "AF-I Nikkor 600mm f/4D IF-ED + TC-20E"),
    ([0x2A,0x54,0x3C,0x3C,0x0C,0x0C,0x26,0x02], "AF Nikkor 28mm f/1.4D"),
    ([0x2B,0x3C,0x44,0x60,0x30,0x3C,0x1F,0x02], "AF Zoom-Nikkor 35-80mm f/4-5.6D"),
    ([0x2C,0x48,0x6A,0x6A,0x18,0x18,0x27,0x02], "AF DC-Nikkor 105mm f/2D"),
    ([0x2D,0x48,0x80,0x80,0x30,0x30,0x21,0x02], "AF Micro-Nikkor 200mm f/4D IF-ED"),
    ([0x2E,0x48,0x5C,0x82,0x30,0x3C,0x22,0x02], "AF Nikkor 70-210mm f/4-5.6D"),
    ([0x2E,0x48,0x5C,0x82,0x30,0x3C,0x28,0x02], "AF Nikkor 70-210mm f/4-5.6D"),
    ([0x30,0x48,0x98,0x98,0x24,0x24,0x24,0x02], "AF-I Nikkor 400mm f/2.8D IF-ED"),
    ([0x30,0x48,0x98,0x98,0x24,0x24,0xF1,0x02], "AF-I Nikkor 400mm f/2.8D IF-ED + TC-14E"),
    ([0x30,0x48,0x98,0x98,0x24,0x24,0xE1,0x02], "AF-I Nikkor 400mm f/2.8D IF-ED + TC-17E"),
    ([0x30,0x48,0x98,0x98,0x24,0x24,0xF2,0x02], "AF-I Nikkor 400mm f/2.8D IF-ED + TC-20E"),
    ([0x31,0x54,0x56,0x56,0x24,0x24,0x25,0x02], "AF Micro-Nikkor 60mm f/2.8D"),
    ([0x33,0x48,0x2D,0x2D,0x24,0x24,0x31,0x02], "AF Nikkor 18mm f/2.8D"),
    ([0x34,0x48,0x29,0x29,0x24,0x24,0x32,0x02], "AF Fisheye Nikkor 16mm f/2.8D"),
    ([0x35,0x3C,0xA0,0xA0,0x30,0x30,0x33,0x02], "AF-I Nikkor 500mm f/4D IF-ED"),
    ([0x35,0x3C,0xA0,0xA0,0x30,0x30,0xF1,0x02], "AF-I Nikkor 500mm f/4D IF-ED + TC-14E"),
    ([0x35,0x3C,0xA0,0xA0,0x30,0x30,0xE1,0x02], "AF-I Nikkor 500mm f/4D IF-ED + TC-17E"),
    ([0x35,0x3C,0xA0,0xA0,0x30,0x30,0xF2,0x02], "AF-I Nikkor 500mm f/4D IF-ED + TC-20E"),
    ([0x36,0x48,0x37,0x37,0x24,0x24,0x34,0x02], "AF Nikkor 24mm f/2.8D"),
    ([0x37,0x48,0x30,0x30,0x24,0x24,0x36,0x02], "AF Nikkor 20mm f/2.8D"),
    ([0x38,0x4C,0x62,0x62,0x14,0x14,0x37,0x02], "AF Nikkor 85mm f/1.8D"),
    ([0x3A,0x40,0x3C,0x5C,0x2C,0x34,0x39,0x02], "AF Zoom-Nikkor 28-70mm f/3.5-4.5D"),
    ([0x3B,0x48,0x44,0x5C,0x24,0x24,0x3A,0x02], "AF Zoom-Nikkor 35-70mm f/2.8D N"),
    ([0x3C,0x48,0x60,0x80,0x24,0x24,0x3B,0x02], "AF Zoom-Nikkor 80-200mm f/2.8D ED"),
    ([0x3D,0x3C,0x44,0x60,0x30,0x3C,0x3E,0x02], "AF Zoom-Nikkor 35-80mm f/4-5.6D"),
    ([0x3E,0x48,0x3C,0x3C,0x24,0x24,0x3D,0x02], "AF Nikkor 28mm f/2.8D"),
    ([0x3F,0x40,0x44,0x6A,0x2C,0x34,0x45,0x02], "AF Zoom-Nikkor 35-105mm f/3.5-4.5D"),
    ([0x41,0x48,0x7C,0x7C,0x24,0x24,0x43,0x02], "AF Nikkor 180mm f/2.8D IF-ED"),
    ([0x42,0x54,0x44,0x44,0x18,0x18,0x44,0x02], "AF Nikkor 35mm f/2D"),
    ([0x43,0x54,0x50,0x50,0x0C,0x0C,0x46,0x02], "AF Nikkor 50mm f/1.4D"),
    ([0x44,0x44,0x60,0x80,0x34,0x3C,0x47,0x02], "AF Zoom-Nikkor 80-200mm f/4.5-5.6D"),
    ([0x45,0x40,0x3C,0x60,0x2C,0x3C,0x48,0x02], "AF Zoom-Nikkor 28-80mm f/3.5-5.6D"),
    ([0x46,0x3C,0x44,0x60,0x30,0x3C,0x49,0x02], "AF Zoom-Nikkor 35-80mm f/4-5.6D N"),
    ([0x47,0x42,0x37,0x50,0x2A,0x34,0x4A,0x02], "AF Zoom-Nikkor 24-50mm f/3.3-4.5D"),
    ([0x48,0x48,0x8E,0x8E,0x24,0x24,0x4B,0x02], "AF-S Nikkor 300mm f/2.8D IF-ED"),
    ([0x48,0x48,0x8E,0x8E,0x24,0x24,0xF1,0x02], "AF-S Nikkor 300mm f/2.8D IF-ED + TC-14E"),
    ([0x48,0x48,0x8E,0x8E,0x24,0x24,0xE1,0x02], "AF-S Nikkor 300mm f/2.8D IF-ED + TC-17E"),
    ([0x48,0x48,0x8E,0x8E,0x24,0x24,0xF2,0x02], "AF-S Nikkor 300mm f/2.8D IF-ED + TC-20E"),
    ([0x49,0x3C,0xA6,0xA6,0x30,0x30,0x4C,0x02], "AF-S Nikkor 600mm f/4D IF-ED"),
    ([0x49,0x3C,0xA6,0xA6,0x30,0x30,0xF1,0x02], "AF-S Nikkor 600mm f/4D IF-ED + TC-14E"),
    ([0x49,0x3C,0xA6,0xA6,0x30,0x30,0xE1,0x02], "AF-S Nikkor 600mm f/4D IF-ED + TC-17E"),
    ([0x49,0x3C,0xA6,0xA6,0x30,0x30,0xF2,0x02], "AF-S Nikkor 600mm f/4D IF-ED + TC-20E"),
    ([0x4A,0x54,0x62,0x62,0x0C,0x0C,0x4D,0x02], "AF Nikkor 85mm f/1.4D IF"),
    ([0x4B,0x3C,0xA0,0xA0,0x30,0x30,0x4E,0x02], "AF-S Nikkor 500mm f/4D IF-ED"),
    ([0x4B,0x3C,0xA0,0xA0,0x30,0x30,0xF1,0x02], "AF-S Nikkor 500mm f/4D IF-ED + TC-14E"),
    ([0x4B,0x3C,0xA0,0xA0,0x30,0x30,0xE1,0x02], "AF-S Nikkor 500mm f/4D IF-ED + TC-17E"),
    ([0x4B,0x3C,0xA0,0xA0,0x30,0x30,0xF2,0x02], "AF-S Nikkor 500mm f/4D IF-ED + TC-20E"),
    ([0x4C,0x40,0x37,0x6E,0x2C,0x3C,0x4F,0x02], "AF Zoom-Nikkor 24-120mm f/3.5-5.6D IF"),
    ([0x4D,0x40,0x3C,0x80,0x2C,0x3C,0x62,0x02], "AF Zoom-Nikkor 28-200mm f/3.5-5.6D IF"),
    ([0x4E,0x48,0x72,0x72,0x18,0x18,0x51,0x02], "AF DC-Nikkor 135mm f/2D"),
    ([0x4F,0x40,0x37,0x5C,0x2C,0x3C,0x53,0x06], "IX-Nikkor 24-70mm f/3.5-5.6"),
    ([0x50,0x48,0x56,0x7C,0x30,0x3C,0x54,0x06], "IX-Nikkor 60-180mm f/4-5.6"),
    ([0x53,0x48,0x60,0x80,0x24,0x24,0x57,0x02], "AF Zoom-Nikkor 80-200mm f/2.8D ED"),
    ([0x53,0x48,0x60,0x80,0x24,0x24,0x60,0x02], "AF Zoom-Nikkor 80-200mm f/2.8D ED"),
    ([0x54,0x44,0x5C,0x7C,0x34,0x3C,0x58,0x02], "AF Zoom-Micro Nikkor 70-180mm f/4.5-5.6D ED"),
    ([0x54,0x44,0x5C,0x7C,0x34,0x3C,0x61,0x02], "AF Zoom-Micro Nikkor 70-180mm f/4.5-5.6D ED"),
    ([0x56,0x48,0x5C,0x8E,0x30,0x3C,0x5A,0x02], "AF Zoom-Nikkor 70-300mm f/4-5.6D ED"),
    ([0x59,0x48,0x98,0x98,0x24,0x24,0x5D,0x02], "AF-S Nikkor 400mm f/2.8D IF-ED"),
    ([0x59,0x48,0x98,0x98,0x24,0x24,0xF1,0x02], "AF-S Nikkor 400mm f/2.8D IF-ED + TC-14E"),
    ([0x59,0x48,0x98,0x98,0x24,0x24,0xE1,0x02], "AF-S Nikkor 400mm f/2.8D IF-ED + TC-17E"),
    ([0x59,0x48,0x98,0x98,0x24,0x24,0xF2,0x02], "AF-S Nikkor 400mm f/2.8D IF-ED + TC-20E"),
    ([0x5A,0x3C,0x3E,0x56,0x30,0x3C,0x5E,0x06], "IX-Nikkor 30-60mm f/4-5.6"),
    ([0x5B,0x44,0x56,0x7C,0x34,0x3C,0x5F,0x06], "IX-Nikkor 60-180mm f/4.5-5.6"),
    ([0x5D,0x48,0x3C,0x5C,0x24,0x24,0x63,0x02], "AF-S Zoom-Nikkor 28-70mm f/2.8D IF-ED"),
    ([0x5E,0x48,0x60,0x80,0x24,0x24,0x64,0x02], "AF-S Zoom-Nikkor 80-200mm f/2.8D IF-ED"),
    ([0x5F,0x40,0x3C,0x6A,0x2C,0x34,0x65,0x02], "AF Zoom-Nikkor 28-105mm f/3.5-4.5D IF"),
    ([0x60,0x40,0x3C,0x60,0x2C,0x3C,0x66,0x02], "AF Zoom-Nikkor 28-80mm f/3.5-5.6D"),
    ([0x61,0x44,0x5E,0x86,0x34,0x3C,0x67,0x02], "AF Zoom-Nikkor 75-240mm f/4.5-5.6D"),
    ([0x63,0x48,0x2B,0x44,0x24,0x24,0x68,0x02], "AF-S Nikkor 17-35mm f/2.8D IF-ED"),
    ([0x64,0x00,0x62,0x62,0x24,0x24,0x6A,0x02], "PC Micro-Nikkor 85mm f/2.8D"),
    ([0x65,0x44,0x60,0x98,0x34,0x3C,0x6B,0x0A], "AF VR Zoom-Nikkor 80-400mm f/4.5-5.6D ED"),
    ([0x66,0x40,0x2D,0x44,0x2C,0x34,0x6C,0x02], "AF Zoom-Nikkor 18-35mm f/3.5-4.5D IF-ED"),
    ([0x67,0x48,0x37,0x62,0x24,0x30,0x6D,0x02], "AF Zoom-Nikkor 24-85mm f/2.8-4D IF"),
    ([0x68,0x42,0x3C,0x60,0x2A,0x3C,0x6E,0x06], "AF Zoom-Nikkor 28-80mm f/3.3-5.6G"),
    ([0x69,0x48,0x5C,0x8E,0x30,0x3C,0x6F,0x06], "AF Zoom-Nikkor 70-300mm f/4-5.6G"),
    ([0x6A,0x48,0x8E,0x8E,0x30,0x30,0x70,0x02], "AF-S Nikkor 300mm f/4D IF-ED"),
    ([0x6B,0x48,0x24,0x24,0x24,0x24,0x71,0x02], "AF Nikkor ED 14mm f/2.8D"),
    ([0x6D,0x48,0x8E,0x8E,0x24,0x24,0x73,0x02], "AF-S Nikkor 300mm f/2.8D IF-ED II"),
    ([0x6E,0x48,0x98,0x98,0x24,0x24,0x74,0x02], "AF-S Nikkor 400mm f/2.8D IF-ED II"),
    ([0x6F,0x3C,0xA0,0xA0,0x30,0x30,0x75,0x02], "AF-S Nikkor 500mm f/4D IF-ED II"),
    ([0x70,0x3C,0xA6,0xA6,0x30,0x30,0x76,0x02], "AF-S Nikkor 600mm f/4D IF-ED II"),
    ([0x72,0x48,0x4C,0x4C,0x24,0x24,0x77,0x00], "Nikkor 45mm f/2.8 P"),
    ([0x74,0x40,0x37,0x62,0x2C,0x34,0x78,0x06], "AF-S Zoom-Nikkor 24-85mm f/3.5-4.5G IF-ED"),
    ([0x75,0x40,0x3C,0x68,0x2C,0x3C,0x79,0x06], "AF Zoom-Nikkor 28-100mm f/3.5-5.6G"),
    ([0x76,0x58,0x50,0x50,0x14,0x14,0x7A,0x02], "AF Nikkor 50mm f/1.8D"),
    ([0x77,0x48,0x5C,0x80,0x24,0x24,0x7B,0x0E], "AF-S VR Zoom-Nikkor 70-200mm f/2.8G IF-ED"),
    ([0x78,0x40,0x37,0x6E,0x2C,0x3C,0x7C,0x0E], "AF-S VR Zoom-Nikkor 24-120mm f/3.5-5.6G IF-ED"),
    ([0x79,0x40,0x3C,0x80,0x2C,0x3C,0x7F,0x06], "AF Zoom-Nikkor 28-200mm f/3.5-5.6G IF-ED"),
    ([0x7B,0x48,0x80,0x98,0x30,0x30,0x80,0x0E], "AF-S VR Zoom-Nikkor 200-400mm f/4G IF-ED"),
    ([0x7D,0x48,0x2B,0x53,0x24,0x24,0x82,0x06], "AF-S DX Zoom-Nikkor 17-55mm f/2.8G IF-ED"),
    ([0x7F,0x40,0x2D,0x5C,0x2C,0x34,0x84,0x06], "AF-S DX Zoom-Nikkor 18-70mm f/3.5-4.5G IF-ED"),
    ([0x80,0x48,0x1A,0x1A,0x24,0x24,0x85,0x06], "AF DX Fisheye-Nikkor 10.5mm f/2.8G ED"),
    ([0x81,0x54,0x80,0x80,0x18,0x18,0x86,0x0E], "AF-S VR Nikkor 200mm f/2G IF-ED"),
    ([0x82,0x48,0x8E,0x8E,0x24,0x24,0x87,0x0E], "AF-S VR Nikkor 300mm f/2.8G IF-ED"),
    ([0x83,0x00,0xB0,0xB0,0x5A,0x5A,0x88,0x04], "FSA-L2, EDG 65, 800mm F13 G"),
    ([0x89,0x3C,0x53,0x80,0x30,0x3C,0x8B,0x06], "AF-S DX Zoom-Nikkor 55-200mm f/4-5.6G ED"),
    ([0x8A,0x54,0x6A,0x6A,0x24,0x24,0x8C,0x0E], "AF-S VR Micro-Nikkor 105mm f/2.8G IF-ED"),
    ([0x8B,0x40,0x2D,0x80,0x2C,0x3C,0x8D,0x0E], "AF-S DX VR Zoom-Nikkor 18-200mm f/3.5-5.6G IF-ED"),
    ([0x8B,0x40,0x2D,0x80,0x2C,0x3C,0xFD,0x0E], "AF-S DX VR Zoom-Nikkor 18-200mm f/3.5-5.6G IF-ED [II]"),
    ([0x8C,0x40,0x2D,0x53,0x2C,0x3C,0x8E,0x06], "AF-S DX Zoom-Nikkor 18-55mm f/3.5-5.6G ED"),
    ([0x8D,0x44,0x5C,0x8E,0x34,0x3C,0x8F,0x0E], "AF-S VR Zoom-Nikkor 70-300mm f/4.5-5.6G IF-ED"),
    ([0x8F,0x40,0x2D,0x72,0x2C,0x3C,0x91,0x06], "AF-S DX Zoom-Nikkor 18-135mm f/3.5-5.6G IF-ED"),
    ([0x90,0x3B,0x53,0x80,0x30,0x3C,0x92,0x0E], "AF-S DX VR Zoom-Nikkor 55-200mm f/4-5.6G IF-ED"),
    ([0x92,0x48,0x24,0x37,0x24,0x24,0x94,0x06], "AF-S Zoom-Nikkor 14-24mm f/2.8G ED"),
    ([0x93,0x48,0x37,0x5C,0x24,0x24,0x95,0x06], "AF-S Zoom-Nikkor 24-70mm f/2.8G ED"),
    ([0x94,0x40,0x2D,0x53,0x2C,0x3C,0x96,0x06], "AF-S DX Zoom-Nikkor 18-55mm f/3.5-5.6G ED II"),
    ([0x95,0x4C,0x37,0x37,0x2C,0x2C,0x97,0x02], "PC-E Nikkor 24mm f/3.5D ED"),
    ([0x95,0x00,0x37,0x37,0x2C,0x2C,0x97,0x06], "PC-E Nikkor 24mm f/3.5D ED"),
    ([0x96,0x48,0x98,0x98,0x24,0x24,0x98,0x0E], "AF-S VR Nikkor 400mm f/2.8G ED"),
    ([0x97,0x3C,0xA0,0xA0,0x30,0x30,0x99,0x0E], "AF-S VR Nikkor 500mm f/4G ED"),
    ([0x98,0x3C,0xA6,0xA6,0x30,0x30,0x9A,0x0E], "AF-S VR Nikkor 600mm f/4G ED"),
    ([0x99,0x40,0x29,0x62,0x2C,0x3C,0x9B,0x0E], "AF-S DX VR Zoom-Nikkor 16-85mm f/3.5-5.6G ED"),
    ([0x9A,0x40,0x2D,0x53,0x2C,0x3C,0x9C,0x0E], "AF-S DX VR Zoom-Nikkor 18-55mm f/3.5-5.6G"),
    ([0x9B,0x54,0x4C,0x4C,0x24,0x24,0x9D,0x02], "PC-E Micro Nikkor 45mm f/2.8D ED"),
    ([0x9B,0x00,0x4C,0x4C,0x24,0x24,0x9D,0x06], "PC-E Micro Nikkor 45mm f/2.8D ED"),
    ([0x9C,0x54,0x56,0x56,0x24,0x24,0x9E,0x06], "AF-S Micro Nikkor 60mm f/2.8G ED"),
    ([0x9D,0x54,0x62,0x62,0x24,0x24,0x9F,0x02], "PC-E Micro Nikkor 85mm f/2.8D"),
    ([0x9D,0x00,0x62,0x62,0x24,0x24,0x9F,0x06], "PC-E Micro Nikkor 85mm f/2.8D"),
    ([0x9E,0x40,0x2D,0x6A,0x2C,0x3C,0xA0,0x0E], "AF-S DX VR Zoom-Nikkor 18-105mm f/3.5-5.6G ED"),
    ([0x9F,0x58,0x44,0x44,0x14,0x14,0xA1,0x06], "AF-S DX Nikkor 35mm f/1.8G"),
    ([0xA0,0x54,0x50,0x50,0x0C,0x0C,0xA2,0x06], "AF-S Nikkor 50mm f/1.4G"),
    ([0xA1,0x40,0x18,0x37,0x2C,0x34,0xA3,0x06], "AF-S DX Nikkor 10-24mm f/3.5-4.5G ED"),
    ([0xA1,0x40,0x2D,0x53,0x2C,0x3C,0xCB,0x86], "AF-P DX Nikkor 18-55mm f/3.5-5.6G"),
    ([0xA2,0x48,0x5C,0x80,0x24,0x24,0xA4,0x0E], "AF-S Nikkor 70-200mm f/2.8G ED VR II"),
    ([0xA3,0x3C,0x29,0x44,0x30,0x30,0xA5,0x0E], "AF-S Nikkor 16-35mm f/4G ED VR"),
    ([0xA4,0x54,0x37,0x37,0x0C,0x0C,0xA6,0x06], "AF-S Nikkor 24mm f/1.4G ED"),
    ([0xA5,0x40,0x3C,0x8E,0x2C,0x3C,0xA7,0x0E], "AF-S Nikkor 28-300mm f/3.5-5.6G ED VR"),
    ([0xA6,0x48,0x8E,0x8E,0x24,0x24,0xA8,0x0E], "AF-S Nikkor 300mm f/2.8G IF-ED VR II"),
    ([0xA7,0x4B,0x62,0x62,0x2C,0x2C,0xA9,0x0E], "AF-S DX Micro Nikkor 85mm f/3.5G ED VR"),
    ([0xA8,0x48,0x80,0x98,0x30,0x30,0xAA,0x0E], "AF-S Zoom-Nikkor 200-400mm f/4G IF-ED VR II"),
    ([0xA9,0x54,0x80,0x80,0x18,0x18,0xAB,0x0E], "AF-S Nikkor 200mm f/2G ED VR II"),
    ([0xAA,0x3C,0x37,0x6E,0x30,0x30,0xAC,0x0E], "AF-S Nikkor 24-120mm f/4G ED VR"),
    ([0xAC,0x38,0x53,0x8E,0x34,0x3C,0xAE,0x0E], "AF-S DX Nikkor 55-300mm f/4.5-5.6G ED VR"),
    ([0xAD,0x3C,0x2D,0x8E,0x2C,0x3C,0xAF,0x0E], "AF-S DX Nikkor 18-300mm f/3.5-5.6G ED VR"),
    ([0xAE,0x54,0x62,0x62,0x0C,0x0C,0xB0,0x06], "AF-S Nikkor 85mm f/1.4G"),
    ([0xAF,0x54,0x44,0x44,0x0C,0x0C,0xB1,0x06], "AF-S Nikkor 35mm f/1.4G"),
    ([0xB0,0x4C,0x50,0x50,0x14,0x14,0xB2,0x06], "AF-S Nikkor 50mm f/1.8G"),
    ([0xB1,0x48,0x48,0x48,0x24,0x24,0xB3,0x06], "AF-S DX Micro Nikkor 40mm f/2.8G"),
    ([0xB2,0x48,0x5C,0x80,0x30,0x30,0xB4,0x0E], "AF-S Nikkor 70-200mm f/4G ED VR"),
    ([0xB3,0x4C,0x62,0x62,0x14,0x14,0xB5,0x06], "AF-S Nikkor 85mm f/1.8G"),
    ([0xB4,0x40,0x37,0x62,0x2C,0x34,0xB6,0x0E], "AF-S Zoom-Nikkor 24-85mm f/3.5-4.5G IF-ED VR"),
    ([0xB5,0x4C,0x3C,0x3C,0x14,0x14,0xB7,0x06], "AF-S Nikkor 28mm f/1.8G"),
    ([0xB6,0x3C,0xB0,0xB0,0x3C,0x3C,0xB8,0x0E], "AF-S VR Nikkor 800mm f/5.6E FL ED"),
    ([0xB6,0x3C,0xB0,0xB0,0x3C,0x3C,0xB8,0x4E], "AF-S VR Nikkor 800mm f/5.6E FL ED"),
    ([0xB7,0x44,0x60,0x98,0x34,0x3C,0xB9,0x0E], "AF-S Nikkor 80-400mm f/4.5-5.6G ED VR"),
    ([0xB8,0x40,0x2D,0x44,0x2C,0x34,0xBA,0x06], "AF-S Nikkor 18-35mm f/3.5-4.5G ED"),
    ([0xA0,0x40,0x2D,0x74,0x2C,0x3C,0xBB,0x0E], "AF-S DX Nikkor 18-140mm f/3.5-5.6G ED VR"),
    ([0xA1,0x54,0x55,0x55,0x0C,0x0C,0xBC,0x06], "AF-S Nikkor 58mm f/1.4G"),
    ([0xA1,0x48,0x6E,0x8E,0x24,0x24,0xDB,0x4E], "AF-S Nikkor 120-300mm f/2.8E FL ED SR VR"),
    ([0xA2,0x40,0x2D,0x53,0x2C,0x3C,0xBD,0x0E], "AF-S DX Nikkor 18-55mm f/3.5-5.6G VR II"),
    ([0xA4,0x40,0x2D,0x8E,0x2C,0x40,0xBF,0x0E], "AF-S DX Nikkor 18-300mm f/3.5-6.3G ED VR"),
    ([0xA5,0x4C,0x44,0x44,0x14,0x14,0xC0,0x06], "AF-S Nikkor 35mm f/1.8G ED"),
    ([0xA6,0x48,0x98,0x98,0x24,0x24,0xC1,0x0E], "AF-S Nikkor 400mm f/2.8E FL ED VR"),
    ([0xA7,0x3C,0x53,0x80,0x30,0x3C,0xC2,0x0E], "AF-S DX Nikkor 55-200mm f/4-5.6G ED VR II"),
    ([0xA8,0x48,0x8E,0x8E,0x30,0x30,0xC3,0x4E], "AF-S Nikkor 300mm f/4E PF ED VR"),
    ([0xA8,0x48,0x8E,0x8E,0x30,0x30,0xC3,0x0E], "AF-S Nikkor 300mm f/4E PF ED VR"),
    ([0xA9,0x4C,0x31,0x31,0x14,0x14,0xC4,0x06], "AF-S Nikkor 20mm f/1.8G ED"),
    ([0xAA,0x48,0x37,0x5C,0x24,0x24,0xC5,0x4E], "AF-S Nikkor 24-70mm f/2.8E ED VR"),
    ([0xAA,0x48,0x37,0x5C,0x24,0x24,0xC5,0x0E], "AF-S Nikkor 24-70mm f/2.8E ED VR"),
    ([0xAB,0x3C,0xA0,0xA0,0x30,0x30,0xC6,0x4E], "AF-S Nikkor 500mm f/4E FL ED VR"),
    ([0xAC,0x3C,0xA6,0xA6,0x30,0x30,0xC7,0x4E], "AF-S Nikkor 600mm f/4E FL ED VR"),
    ([0xAD,0x48,0x28,0x60,0x24,0x30,0xC8,0x4E], "AF-S DX Nikkor 16-80mm f/2.8-4E ED VR"),
    ([0xAD,0x48,0x28,0x60,0x24,0x30,0xC8,0x0E], "AF-S DX Nikkor 16-80mm f/2.8-4E ED VR"),
    ([0xAE,0x3C,0x80,0xA0,0x3C,0x3C,0xC9,0x4E], "AF-S Nikkor 200-500mm f/5.6E ED VR"),
    ([0xAE,0x3C,0x80,0xA0,0x3C,0x3C,0xC9,0x0E], "AF-S Nikkor 200-500mm f/5.6E ED VR"),
    ([0xA0,0x40,0x2D,0x53,0x2C,0x3C,0xCA,0x8E], "AF-P DX Nikkor 18-55mm f/3.5-5.6G"),
    ([0xA0,0x40,0x2D,0x53,0x2C,0x3C,0xCA,0x0E], "AF-P DX Nikkor 18-55mm f/3.5-5.6G VR"),
    ([0xAF,0x4C,0x37,0x37,0x14,0x14,0xCC,0x06], "AF-S Nikkor 24mm f/1.8G ED"),
    ([0xA2,0x38,0x5C,0x8E,0x34,0x40,0xCD,0x86], "AF-P DX Nikkor 70-300mm f/4.5-6.3G VR"),
    ([0xA3,0x38,0x5C,0x8E,0x34,0x40,0xCE,0x8E], "AF-P DX Nikkor 70-300mm f/4.5-6.3G ED VR"),
    ([0xA3,0x38,0x5C,0x8E,0x34,0x40,0xCE,0x0E], "AF-P DX Nikkor 70-300mm f/4.5-6.3G ED"),
    ([0xA4,0x48,0x5C,0x80,0x24,0x24,0xCF,0x4E], "AF-S Nikkor 70-200mm f/2.8E FL ED VR"),
    ([0xA4,0x48,0x5C,0x80,0x24,0x24,0xCF,0x0E], "AF-S Nikkor 70-200mm f/2.8E FL ED VR"),
    ([0xA5,0x54,0x6A,0x6A,0x0C,0x0C,0xD0,0x46], "AF-S Nikkor 105mm f/1.4E ED"),
    ([0xA5,0x54,0x6A,0x6A,0x0C,0x0C,0xD0,0x06], "AF-S Nikkor 105mm f/1.4E ED"),
    ([0xA6,0x48,0x2F,0x2F,0x30,0x30,0xD1,0x46], "PC Nikkor 19mm f/4E ED"),
    ([0xA6,0x48,0x2F,0x2F,0x30,0x30,0xD1,0x06], "PC Nikkor 19mm f/4E ED"),
    ([0xA7,0x40,0x11,0x26,0x2C,0x34,0xD2,0x46], "AF-S Fisheye Nikkor 8-15mm f/3.5-4.5E ED"),
    ([0xA7,0x40,0x11,0x26,0x2C,0x34,0xD2,0x06], "AF-S Fisheye Nikkor 8-15mm f/3.5-4.5E ED"),
    ([0xA8,0x38,0x18,0x30,0x34,0x3C,0xD3,0x8E], "AF-P DX Nikkor 10-20mm f/4.5-5.6G VR"),
    ([0xA8,0x38,0x18,0x30,0x34,0x3C,0xD3,0x0E], "AF-P DX Nikkor 10-20mm f/4.5-5.6G VR"),
    ([0xA9,0x48,0x7C,0x98,0x30,0x30,0xD4,0x4E], "AF-S Nikkor 180-400mm f/4E TC1.4 FL ED VR"),
    ([0xA9,0x48,0x7C,0x98,0x30,0x30,0xD4,0x0E], "AF-S Nikkor 180-400mm f/4E TC1.4 FL ED VR"),
    ([0xAA,0x48,0x88,0xA4,0x3C,0x3C,0xD5,0x4E], "AF-S Nikkor 180-400mm f/4E TC1.4 FL ED VR + 1.4x TC"),
    ([0xAA,0x48,0x88,0xA4,0x3C,0x3C,0xD5,0x0E], "AF-S Nikkor 180-400mm f/4E TC1.4 FL ED VR + 1.4x TC"),
    ([0xAB,0x44,0x5C,0x8E,0x34,0x3C,0xD6,0xCE], "AF-P Nikkor 70-300mm f/4.5-5.6E ED VR"),
    ([0xAB,0x44,0x5C,0x8E,0x34,0x3C,0xD6,0x0E], "AF-P Nikkor 70-300mm f/4.5-5.6E ED VR"),
    ([0xAB,0x44,0x5C,0x8E,0x34,0x3C,0xD6,0x4E], "AF-P Nikkor 70-300mm f/4.5-5.6E ED VR"),
    ([0xAC,0x54,0x3C,0x3C,0x0C,0x0C,0xD7,0x46], "AF-S Nikkor 28mm f/1.4E ED"),
    ([0xAC,0x54,0x3C,0x3C,0x0C,0x0C,0xD7,0x06], "AF-S Nikkor 28mm f/1.4E ED"),
    ([0xAD,0x3C,0xA0,0xA0,0x3C,0x3C,0xD8,0x0E], "AF-S Nikkor 500mm f/5.6E PF ED VR"),
    ([0xAD,0x3C,0xA0,0xA0,0x3C,0x3C,0xD8,0x4E], "AF-S Nikkor 500mm f/5.6E PF ED VR"),
    ([0x01,0x00,0x00,0x00,0x00,0x00,0x02,0x00], "TC-16A"),
    ([0x01,0x00,0x00,0x00,0x00,0x00,0x08,0x00], "TC-16A"),
    ([0x00,0x00,0x00,0x00,0x00,0x00,0xF1,0x0C], "TC-14E [II] or Sigma APO Tele Converter 1.4x EX DG or Kenko Teleplus PRO 300 DG 1.4x"),
    ([0x00,0x00,0x00,0x00,0x00,0x00,0xF2,0x18], "TC-20E [II] or Sigma APO Tele Converter 2x EX DG or Kenko Teleplus PRO 300 DG 2.0x"),
    ([0x00,0x00,0x00,0x00,0x00,0x00,0xE1,0x12], "TC-17E II"),
    ([0xFE,0x47,0x00,0x00,0x24,0x24,0x4B,0x06], "Sigma 4.5mm F2.8 EX DC HSM Circular Fisheye"),
    ([0x26,0x48,0x11,0x11,0x30,0x30,0x1C,0x02], "Sigma 8mm F4 EX Circular Fisheye"),
    ([0x79,0x40,0x11,0x11,0x2C,0x2C,0x1C,0x06], "Sigma 8mm F3.5 EX Circular Fisheye"),
    ([0xDB,0x40,0x11,0x11,0x2C,0x2C,0x1C,0x06], "Sigma 8mm F3.5 EX DG Circular Fisheye"),
    ([0xDC,0x48,0x19,0x19,0x24,0x24,0x4B,0x06], "Sigma 10mm F2.8 EX DC HSM Fisheye"),
    ([0xC2,0x4C,0x24,0x24,0x14,0x14,0x4B,0x06], "Sigma 14mm F1.8 DG HSM | A"),
    ([0x48,0x48,0x24,0x24,0x24,0x24,0x4B,0x02], "Sigma 14mm F2.8 EX Aspherical HSM"),
    ([0x02,0x3F,0x24,0x24,0x2C,0x2C,0x02,0x00], "Sigma 14mm F3.5"),
    ([0x26,0x48,0x27,0x27,0x24,0x24,0x1C,0x02], "Sigma 15mm F2.8 EX Diagonal Fisheye"),
    ([0xEA,0x48,0x27,0x27,0x24,0x24,0x1C,0x02], "Sigma 15mm F2.8 EX Diagonal Fisheye"),
    ([0x26,0x58,0x31,0x31,0x14,0x14,0x1C,0x02], "Sigma 20mm F1.8 EX DG Aspherical RF"),
    ([0x79,0x54,0x31,0x31,0x0C,0x0C,0x4B,0x06], "Sigma 20mm F1.4 DG HSM | A"),
    ([0x26,0x58,0x37,0x37,0x14,0x14,0x1C,0x02], "Sigma 24mm F1.8 EX DG Aspherical Macro"),
    ([0xE1,0x58,0x37,0x37,0x14,0x14,0x1C,0x02], "Sigma 24mm F1.8 EX DG Aspherical Macro"),
    ([0x02,0x46,0x37,0x37,0x25,0x25,0x02,0x00], "Sigma 24mm F2.8 Super Wide II Macro"),
    ([0x7E,0x54,0x37,0x37,0x0C,0x0C,0x4B,0x06], "Sigma 24mm F1.4 DG HSM | A"),
    ([0x26,0x58,0x3C,0x3C,0x14,0x14,0x1C,0x02], "Sigma 28mm F1.8 EX DG Aspherical Macro"),
    ([0xBC,0x54,0x3C,0x3C,0x0C,0x0C,0x4B,0x46], "Sigma 28mm F1.4 DG HSM | A"),
    ([0x48,0x54,0x3E,0x3E,0x0C,0x0C,0x4B,0x06], "Sigma 30mm F1.4 EX DC HSM"),
    ([0xF8,0x54,0x3E,0x3E,0x0C,0x0C,0x4B,0x06], "Sigma 30mm F1.4 EX DC HSM"),
    ([0x91,0x54,0x44,0x44,0x0C,0x0C,0x4B,0x06], "Sigma 35mm F1.4 DG HSM"),
    ([0xBD,0x54,0x48,0x48,0x0C,0x0C,0x4B,0x46], "Sigma 40mm F1.4 DG HSM | A"),
    ([0xDE,0x54,0x50,0x50,0x0C,0x0C,0x4B,0x06], "Sigma 50mm F1.4 EX DG HSM"),
    ([0x88,0x54,0x50,0x50,0x0C,0x0C,0x4B,0x06], "Sigma 50mm F1.4 DG HSM | A"),
    ([0x02,0x48,0x50,0x50,0x24,0x24,0x02,0x00], "Sigma Macro 50mm F2.8"),
    ([0x32,0x54,0x50,0x50,0x24,0x24,0x35,0x02], "Sigma Macro 50mm F2.8 EX DG"),
    ([0xE3,0x54,0x50,0x50,0x24,0x24,0x35,0x02], "Sigma Macro 50mm F2.8 EX DG"),
    ([0x79,0x48,0x5C,0x5C,0x24,0x24,0x1C,0x06], "Sigma Macro 70mm F2.8 EX DG"),
    ([0x9B,0x54,0x62,0x62,0x0C,0x0C,0x4B,0x06], "Sigma 85mm F1.4 EX DG HSM"),
    ([0xC8,0x54,0x62,0x62,0x0C,0x0C,0x4B,0x46], "Sigma 85mm F1.4 DG HSM | A"),
    ([0xC8,0x54,0x62,0x62,0x0C,0x0C,0x4B,0x06], "Sigma 85mm F1.4 DG HSM | A"),
    ([0x02,0x48,0x65,0x65,0x24,0x24,0x02,0x00], "Sigma Macro 90mm F2.8"),
    ([0xE5,0x54,0x6A,0x6A,0x24,0x24,0x35,0x02], "Sigma Macro 105mm F2.8 EX DG"),
    ([0x97,0x48,0x6A,0x6A,0x24,0x24,0x4B,0x0E], "Sigma Macro 105mm F2.8 EX DG OS HSM"),
    ([0xBE,0x54,0x6A,0x6A,0x0C,0x0C,0x4B,0x46], "Sigma 105mm F1.4 DG HSM | A"),
    ([0x48,0x48,0x76,0x76,0x24,0x24,0x4B,0x06], "Sigma APO Macro 150mm F2.8 EX DG HSM"),
    ([0xF5,0x48,0x76,0x76,0x24,0x24,0x4B,0x06], "Sigma APO Macro 150mm F2.8 EX DG HSM"),
    ([0x99,0x48,0x76,0x76,0x24,0x24,0x4B,0x0E], "Sigma APO Macro 150mm F2.8 EX DG OS HSM"),
    ([0x48,0x4C,0x7C,0x7C,0x2C,0x2C,0x4B,0x02], "Sigma APO Macro 180mm F3.5 EX DG HSM"),
    ([0x48,0x4C,0x7D,0x7D,0x2C,0x2C,0x4B,0x02], "Sigma APO Macro 180mm F3.5 EX DG HSM"),
    ([0xF4,0x4C,0x7C,0x7C,0x2C,0x2C,0x4B,0x02], "Sigma APO Macro 180mm F3.5 EX DG HSM"),
    ([0x94,0x48,0x7C,0x7C,0x24,0x24,0x4B,0x0E], "Sigma APO Macro 180mm F2.8 EX DG OS HSM"),
    ([0x48,0x54,0x8E,0x8E,0x24,0x24,0x4B,0x02], "Sigma APO 300mm F2.8 EX DG HSM"),
    ([0xFB,0x54,0x8E,0x8E,0x24,0x24,0x4B,0x02], "Sigma APO 300mm F2.8 EX DG HSM"),
    ([0x26,0x48,0x8E,0x8E,0x30,0x30,0x1C,0x02], "Sigma APO Tele Macro 300mm F4"),
    ([0x02,0x2F,0x98,0x98,0x3D,0x3D,0x02,0x00], "Sigma APO 400mm F5.6"),
    ([0x26,0x3C,0x98,0x98,0x3C,0x3C,0x1C,0x02], "Sigma APO Tele Macro 400mm F5.6"),
    ([0x02,0x37,0xA0,0xA0,0x34,0x34,0x02,0x00], "Sigma APO 500mm F4.5"),
    ([0x48,0x44,0xA0,0xA0,0x34,0x34,0x4B,0x02], "Sigma APO 500mm F4.5 EX HSM"),
    ([0xF1,0x44,0xA0,0xA0,0x34,0x34,0x4B,0x02], "Sigma APO 500mm F4.5 EX DG HSM"),
    ([0x02,0x34,0xA0,0xA0,0x44,0x44,0x02,0x00], "Sigma APO 500mm F7.2"),
    ([0x02,0x3C,0xB0,0xB0,0x3C,0x3C,0x02,0x00], "Sigma APO 800mm F5.6"),
    ([0x48,0x3C,0xB0,0xB0,0x3C,0x3C,0x4B,0x02], "Sigma APO 800mm F5.6 EX HSM"),
    ([0x9E,0x38,0x11,0x29,0x34,0x3C,0x4B,0x06], "Sigma 8-16mm F4.5-5.6 DC HSM"),
    ([0xA1,0x41,0x19,0x31,0x2C,0x2C,0x4B,0x06], "Sigma 10-20mm F3.5 EX DC HSM"),
    ([0x48,0x3C,0x19,0x31,0x30,0x3C,0x4B,0x06], "Sigma 10-20mm F4-5.6 EX DC HSM"),
    ([0xF9,0x3C,0x19,0x31,0x30,0x3C,0x4B,0x06], "Sigma 10-20mm F4-5.6 EX DC HSM"),
    ([0x48,0x38,0x1F,0x37,0x34,0x3C,0x4B,0x06], "Sigma 12-24mm F4.5-5.6 EX DG Aspherical HSM"),
    ([0xF0,0x38,0x1F,0x37,0x34,0x3C,0x4B,0x06], "Sigma 12-24mm F4.5-5.6 EX DG Aspherical HSM"),
    ([0x96,0x38,0x1F,0x37,0x34,0x3C,0x4B,0x06], "Sigma 12-24mm F4.5-5.6 II DG HSM"),
    ([0xCA,0x3C,0x1F,0x37,0x30,0x30,0x4B,0x46], "Sigma 12-24mm F4 DG HSM | A"),
    ([0xC1,0x48,0x24,0x37,0x24,0x24,0x4B,0x46], "Sigma 14-24mm F2.8 DG HSM | A"),
    ([0x26,0x40,0x27,0x3F,0x2C,0x34,0x1C,0x02], "Sigma 15-30mm F3.5-4.5 EX DG Aspherical DF"),
    ([0x48,0x48,0x2B,0x44,0x24,0x30,0x4B,0x06], "Sigma 17-35mm F2.8-4 EX DG  Aspherical HSM"),
    ([0x26,0x54,0x2B,0x44,0x24,0x30,0x1C,0x02], "Sigma 17-35mm F2.8-4 EX Aspherical"),
    ([0x9D,0x48,0x2B,0x50,0x24,0x24,0x4B,0x0E], "Sigma 17-50mm F2.8 EX DC OS HSM"),
    ([0x8F,0x48,0x2B,0x50,0x24,0x24,0x4B,0x0E], "Sigma 17-50mm F2.8 EX DC OS HSM"),
    ([0x7A,0x47,0x2B,0x5C,0x24,0x34,0x4B,0x06], "Sigma 17-70mm F2.8-4.5 DC Macro Asp. IF HSM"),
    ([0x7A,0x48,0x2B,0x5C,0x24,0x34,0x4B,0x06], "Sigma 17-70mm F2.8-4.5 DC Macro Asp. IF HSM"),
    ([0x7F,0x48,0x2B,0x5C,0x24,0x34,0x1C,0x06], "Sigma 17-70mm F2.8-4.5 DC Macro Asp. IF"),
    ([0x8E,0x3C,0x2B,0x5C,0x24,0x30,0x4B,0x0E], "Sigma 17-70mm F2.8-4 DC Macro OS HSM | C"),
    ([0xA0,0x48,0x2A,0x5C,0x24,0x30,0x4B,0x0E], "Sigma 17-70mm F2.8-4 DC Macro OS HSM"),
    ([0x8B,0x4C,0x2D,0x44,0x14,0x14,0x4B,0x06], "Sigma 18-35mm F1.8 DC HSM"),
    ([0x26,0x40,0x2D,0x44,0x2B,0x34,0x1C,0x02], "Sigma 18-35mm F3.5-4.5 Aspherical"),
    ([0x26,0x48,0x2D,0x50,0x24,0x24,0x1C,0x06], "Sigma 18-50mm F2.8 EX DC"),
    ([0x7F,0x48,0x2D,0x50,0x24,0x24,0x1C,0x06], "Sigma 18-50mm F2.8 EX DC Macro"),
    ([0x7A,0x48,0x2D,0x50,0x24,0x24,0x4B,0x06], "Sigma 18-50mm F2.8 EX DC Macro"),
    ([0xF6,0x48,0x2D,0x50,0x24,0x24,0x4B,0x06], "Sigma 18-50mm F2.8 EX DC Macro"),
    ([0xA4,0x47,0x2D,0x50,0x24,0x34,0x4B,0x0E], "Sigma 18-50mm F2.8-4.5 DC OS HSM"),
    ([0x26,0x40,0x2D,0x50,0x2C,0x3C,0x1C,0x06], "Sigma 18-50mm F3.5-5.6 DC"),
    ([0x7A,0x40,0x2D,0x50,0x2C,0x3C,0x4B,0x06], "Sigma 18-50mm F3.5-5.6 DC HSM"),
    ([0x26,0x40,0x2D,0x70,0x2B,0x3C,0x1C,0x06], "Sigma 18-125mm F3.5-5.6 DC"),
    ([0xCD,0x3D,0x2D,0x70,0x2E,0x3C,0x4B,0x0E], "Sigma 18-125mm F3.8-5.6 DC OS HSM"),
    ([0x26,0x40,0x2D,0x80,0x2C,0x40,0x1C,0x06], "Sigma 18-200mm F3.5-6.3 DC"),
    ([0xFF,0x40,0x2D,0x80,0x2C,0x40,0x4B,0x06], "Sigma 18-200mm F3.5-6.3 DC"),
    ([0x7A,0x40,0x2D,0x80,0x2C,0x40,0x4B,0x0E], "Sigma 18-200mm F3.5-6.3 DC OS HSM"),
    ([0xED,0x40,0x2D,0x80,0x2C,0x40,0x4B,0x0E], "Sigma 18-200mm F3.5-6.3 DC OS HSM"),
    ([0x90,0x40,0x2D,0x80,0x2C,0x40,0x4B,0x0E], "Sigma 18-200mm F3.5-6.3 II DC OS HSM"),
    ([0x89,0x30,0x2D,0x80,0x2C,0x40,0x4B,0x0E], "Sigma 18-200mm F3.5-6.3 DC Macro OS HS | C"),
    ([0xA5,0x40,0x2D,0x88,0x2C,0x40,0x4B,0x0E], "Sigma 18-250mm F3.5-6.3 DC OS HSM"),
    ([0x92,0x2C,0x2D,0x88,0x2C,0x40,0x4B,0x0E], "Sigma 18-250mm F3.5-6.3 DC Macro OS HSM"),
    ([0x87,0x2C,0x2D,0x8E,0x2C,0x40,0x4B,0x0E], "Sigma 18-300mm F3.5-6.3 DC Macro HSM"),
    ([0x26,0x48,0x31,0x49,0x24,0x24,0x1C,0x02], "Sigma 20-40mm F2.8"),
    ([0x7B,0x48,0x37,0x44,0x18,0x18,0x4B,0x06], "Sigma 24-35mm F2.0 DG HSM | A"),
    ([0x02,0x3A,0x37,0x50,0x31,0x3D,0x02,0x00], "Sigma 24-50mm F4-5.6 UC"),
    ([0x26,0x48,0x37,0x56,0x24,0x24,0x1C,0x02], "Sigma 24-60mm F2.8 EX DG"),
    ([0xB6,0x48,0x37,0x56,0x24,0x24,0x1C,0x02], "Sigma 24-60mm F2.8 EX DG"),
    ([0xA6,0x48,0x37,0x5C,0x24,0x24,0x4B,0x06], "Sigma 24-70mm F2.8 IF EX DG HSM"),
    ([0xC9,0x48,0x37,0x5C,0x24,0x24,0x4B,0x4E], "Sigma 24-70mm F2.8 DG OS HSM | A"),
    ([0x26,0x54,0x37,0x5C,0x24,0x24,0x1C,0x02], "Sigma 24-70mm F2.8 EX DG Macro"),
    ([0x67,0x54,0x37,0x5C,0x24,0x24,0x1C,0x02], "Sigma 24-70mm F2.8 EX DG Macro"),
    ([0xE9,0x54,0x37,0x5C,0x24,0x24,0x1C,0x02], "Sigma 24-70mm F2.8 EX DG Macro"),
    ([0x26,0x40,0x37,0x5C,0x2C,0x3C,0x1C,0x02], "Sigma 24-70mm F3.5-5.6 Aspherical HF"),
    ([0x8A,0x3C,0x37,0x6A,0x30,0x30,0x4B,0x0E], "Sigma 24-105mm F4 DG OS HSM"),
    ([0x26,0x54,0x37,0x73,0x24,0x34,0x1C,0x02], "Sigma 24-135mm F2.8-4.5"),
    ([0x02,0x46,0x3C,0x5C,0x25,0x25,0x02,0x00], "Sigma 28-70mm F2.8"),
    ([0x26,0x54,0x3C,0x5C,0x24,0x24,0x1C,0x02], "Sigma 28-70mm F2.8 EX"),
    ([0x26,0x48,0x3C,0x5C,0x24,0x24,0x1C,0x06], "Sigma 28-70mm F2.8 EX DG"),
    ([0x79,0x48,0x3C,0x5C,0x24,0x24,0x1C,0x06], "Sigma 28-70mm F2.8 EX DG"),
    ([0x26,0x48,0x3C,0x5C,0x24,0x30,0x1C,0x02], "Sigma 28-70mm F2.8-4 DG"),
    ([0x02,0x3F,0x3C,0x5C,0x2D,0x35,0x02,0x00], "Sigma 28-70mm F3.5-4.5 UC"),
    ([0x26,0x40,0x3C,0x60,0x2C,0x3C,0x1C,0x02], "Sigma 28-80mm F3.5-5.6 Mini Zoom Macro II Aspherical"),
    ([0x26,0x40,0x3C,0x65,0x2C,0x3C,0x1C,0x02], "Sigma 28-90mm F3.5-5.6 Macro"),
    ([0x26,0x48,0x3C,0x6A,0x24,0x30,0x1C,0x02], "Sigma 28-105mm F2.8-4 Aspherical"),
    ([0x26,0x3E,0x3C,0x6A,0x2E,0x3C,0x1C,0x02], "Sigma 28-105mm F3.8-5.6 UC-III Aspherical IF"),
    ([0x26,0x40,0x3C,0x80,0x2C,0x3C,0x1C,0x02], "Sigma 28-200mm F3.5-5.6 Compact Aspherical Hyperzoom Macro"),
    ([0x26,0x40,0x3C,0x80,0x2B,0x3C,0x1C,0x02], "Sigma 28-200mm F3.5-5.6 Compact Aspherical Hyperzoom Macro"),
    ([0x26,0x3D,0x3C,0x80,0x2F,0x3D,0x1C,0x02], "Sigma 28-300mm F3.8-5.6 Aspherical"),
    ([0x26,0x41,0x3C,0x8E,0x2C,0x40,0x1C,0x02], "Sigma 28-300mm F3.5-6.3 DG Macro"),
    ([0xE6,0x41,0x3C,0x8E,0x2C,0x40,0x1C,0x02], "Sigma 28-300mm F3.5-6.3 DG Macro"),
    ([0x26,0x40,0x3C,0x8E,0x2C,0x40,0x1C,0x02], "Sigma 28-300mm F3.5-6.3 Macro"),
    ([0x02,0x3B,0x44,0x61,0x30,0x3D,0x02,0x00], "Sigma 35-80mm F4-5.6"),
    ([0x02,0x40,0x44,0x73,0x2B,0x36,0x02,0x00], "Sigma 35-135mm F3.5-4.5 a"),
    ([0xCC,0x4C,0x50,0x68,0x14,0x14,0x4B,0x06], "Sigma 50-100mm F1.8 DC HSM | A"),
    ([0x7A,0x47,0x50,0x76,0x24,0x24,0x4B,0x06], "Sigma 50-150mm F2.8 EX APO DC HSM"),
    ([0xFD,0x47,0x50,0x76,0x24,0x24,0x4B,0x06], "Sigma 50-150mm F2.8 EX APO DC HSM II"),
    ([0x98,0x48,0x50,0x76,0x24,0x24,0x4B,0x0E], "Sigma 50-150mm F2.8 EX APO DC OS HSM"),
    ([0x48,0x3C,0x50,0xA0,0x30,0x40,0x4B,0x02], "Sigma 50-500mm F4-6.3 EX APO RF HSM"),
    ([0x9F,0x37,0x50,0xA0,0x34,0x40,0x4B,0x0E], "Sigma 50-500mm F4.5-6.3 DG OS HSM"),
    ([0x26,0x3C,0x54,0x80,0x30,0x3C,0x1C,0x06], "Sigma 55-200mm F4-5.6 DC"),
    ([0x7A,0x3B,0x53,0x80,0x30,0x3C,0x4B,0x06], "Sigma 55-200mm F4-5.6 DC HSM"),
    ([0x48,0x54,0x5C,0x80,0x24,0x24,0x4B,0x02], "Sigma 70-200mm F2.8 EX APO IF HSM"),
    ([0x7A,0x48,0x5C,0x80,0x24,0x24,0x4B,0x06], "Sigma 70-200mm F2.8 EX APO DG Macro HSM II"),
    ([0xEE,0x48,0x5C,0x80,0x24,0x24,0x4B,0x06], "Sigma 70-200mm F2.8 EX APO DG Macro HSM II"),
    ([0x9C,0x48,0x5C,0x80,0x24,0x24,0x4B,0x0E], "Sigma 70-200mm F2.8 EX DG OS HSM"),
    ([0xBB,0x48,0x5C,0x80,0x24,0x24,0x4B,0x4E], "Sigma 70-200mm F2.8 DG OS HSM | S"),
    ([0x02,0x46,0x5C,0x82,0x25,0x25,0x02,0x00], "Sigma 70-210mm F2.8 APO"),
    ([0x02,0x40,0x5C,0x82,0x2C,0x35,0x02,0x00], "Sigma APO 70-210mm F3.5-4.5"),
    ([0x26,0x3C,0x5C,0x82,0x30,0x3C,0x1C,0x02], "Sigma 70-210mm F4-5.6 UC-II"),
    ([0x02,0x3B,0x5C,0x82,0x30,0x3C,0x02,0x00], "Sigma Zoom-K 70-210mm F4-5.6"),
    ([0x26,0x3C,0x5C,0x8E,0x30,0x3C,0x1C,0x02], "Sigma 70-300mm F4-5.6 DG Macro"),
    ([0x56,0x3C,0x5C,0x8E,0x30,0x3C,0x1C,0x02], "Sigma 70-300mm F4-5.6 APO Macro Super II"),
    ([0xE0,0x3C,0x5C,0x8E,0x30,0x3C,0x4B,0x06], "Sigma 70-300mm F4-5.6 APO DG Macro HSM"),
    ([0xA3,0x3C,0x5C,0x8E,0x30,0x3C,0x4B,0x0E], "Sigma 70-300mm F4-5.6 DG OS"),
    ([0x02,0x37,0x5E,0x8E,0x35,0x3D,0x02,0x00], "Sigma 75-300mm F4.5-5.6 APO"),
    ([0x02,0x3A,0x5E,0x8E,0x32,0x3D,0x02,0x00], "Sigma 75-300mm F4.0-5.6"),
    ([0x77,0x44,0x61,0x98,0x34,0x3C,0x7B,0x0E], "Sigma 80-400mm F4.5-5.6 EX OS"),
    ([0x77,0x44,0x60,0x98,0x34,0x3C,0x7B,0x0E], "Sigma 80-400mm F4.5-5.6 APO DG D OS"),
    ([0x48,0x48,0x68,0x8E,0x30,0x30,0x4B,0x02], "Sigma APO 100-300mm F4 EX IF HSM"),
    ([0xF3,0x48,0x68,0x8E,0x30,0x30,0x4B,0x02], "Sigma APO 100-300mm F4 EX IF HSM"),
    ([0x26,0x45,0x68,0x8E,0x34,0x42,0x1C,0x02], "Sigma 100-300mm F4.5-6.7 DL"),
    ([0x48,0x54,0x6F,0x8E,0x24,0x24,0x4B,0x02], "Sigma APO 120-300mm F2.8 EX DG HSM"),
    ([0x7A,0x54,0x6E,0x8E,0x24,0x24,0x4B,0x02], "Sigma APO 120-300mm F2.8 EX DG HSM"),
    ([0xFA,0x54,0x6E,0x8E,0x24,0x24,0x4B,0x02], "Sigma APO 120-300mm F2.8 EX DG HSM"),
    ([0xCF,0x38,0x6E,0x98,0x34,0x3C,0x4B,0x0E], "Sigma APO 120-400mm F4.5-5.6 DG OS HSM"),
    ([0xC3,0x34,0x68,0x98,0x38,0x40,0x4B,0x4E], "Sigma 100-400mm F5-6.3 DG OS HSM | C"),
    ([0x8D,0x48,0x6E,0x8E,0x24,0x24,0x4B,0x0E], "Sigma 120-300mm F2.8 DG OS HSM Sports"),
    ([0x26,0x44,0x73,0x98,0x34,0x3C,0x1C,0x02], "Sigma 135-400mm F4.5-5.6 APO Aspherical"),
    ([0xCE,0x34,0x76,0xA0,0x38,0x40,0x4B,0x0E], "Sigma 150-500mm F5-6.3 DG OS APO HSM"),
    ([0x81,0x34,0x76,0xA6,0x38,0x40,0x4B,0x0E], "Sigma 150-600mm F5-6.3 DG OS HSM | S"),
    ([0x82,0x34,0x76,0xA6,0x38,0x40,0x4B,0x0E], "Sigma 150-600mm F5-6.3 DG OS HSM | C"),
    ([0xC4,0x4C,0x73,0x73,0x14,0x14,0x4B,0x46], "Sigma 135mm F1.8 DG HSM | A"),
    ([0x26,0x40,0x7B,0xA0,0x34,0x40,0x1C,0x02], "Sigma APO 170-500mm F5-6.3 Aspherical RF"),
    ([0xA7,0x49,0x80,0xA0,0x24,0x24,0x4B,0x06], "Sigma APO 200-500mm F2.8 EX DG"),
    ([0x48,0x3C,0x8E,0xB0,0x3C,0x3C,0x4B,0x02], "Sigma APO 300-800mm F5.6 EX DG HSM"),
    ([0xD2,0x3C,0x8E,0xB0,0x3C,0x3C,0x4B,0x02], "Sigma APO 300-800mm F5.6 EX DG HSM"),
    ([0x00,0x47,0x25,0x25,0x24,0x24,0x00,0x02], "Tamron SP AF 14mm f/2.8 Aspherical (IF) (69E)"),
    ([0xC8,0x54,0x44,0x44,0x0D,0x0D,0xDF,0x46], "Tamron SP 35mm f/1.4 Di USD (F045)"),
    ([0xE8,0x4C,0x44,0x44,0x14,0x14,0xDF,0x0E], "Tamron SP 35mm f/1.8 Di VC USD (F012)"),
    ([0xE7,0x4C,0x4C,0x4C,0x14,0x14,0xDF,0x0E], "Tamron SP 45mm f/1.8 Di VC USD (F013)"),
    ([0xF4,0x54,0x56,0x56,0x18,0x18,0x84,0x06], "Tamron SP AF 60mm f/2.0 Di II Macro 1:1 (G005)"),
    ([0xE5,0x4C,0x62,0x62,0x14,0x14,0xC9,0x4E], "Tamron SP 85mm f/1.8 Di VC USD (F016)"),
    ([0x1E,0x5D,0x64,0x64,0x20,0x20,0x13,0x00], "Tamron SP AF 90mm f/2.5 (52E)"),
    ([0x20,0x5A,0x64,0x64,0x20,0x20,0x14,0x00], "Tamron SP AF 90mm f/2.5 Macro (152E)"),
    ([0x22,0x53,0x64,0x64,0x24,0x24,0xE0,0x02], "Tamron SP AF 90mm f/2.8 Macro 1:1 (72E)"),
    ([0x32,0x53,0x64,0x64,0x24,0x24,0x35,0x02], "Tamron SP AF 90mm f/2.8 [Di] Macro 1:1 (172E/272E)"),
    ([0xF8,0x55,0x64,0x64,0x24,0x24,0x84,0x06], "Tamron SP AF 90mm f/2.8 Di Macro 1:1 (272NII)"),
    ([0xF8,0x54,0x64,0x64,0x24,0x24,0xDF,0x06], "Tamron SP AF 90mm f/2.8 Di Macro 1:1 (272NII)"),
    ([0xFE,0x54,0x64,0x64,0x24,0x24,0xDF,0x0E], "Tamron SP 90mm f/2.8 Di VC USD Macro 1:1 (F004)"),
    ([0xE4,0x54,0x64,0x64,0x24,0x24,0xDF,0x0E], "Tamron SP 90mm f/2.8 Di VC USD Macro 1:1 (F017)"),
    ([0x00,0x4C,0x7C,0x7C,0x2C,0x2C,0x00,0x02], "Tamron SP AF 180mm f/3.5 Di Model (B01)"),
    ([0x21,0x56,0x8E,0x8E,0x24,0x24,0x14,0x00], "Tamron SP AF 300mm f/2.8 LD-IF (60E)"),
    ([0x27,0x54,0x8E,0x8E,0x24,0x24,0x1D,0x02], "Tamron SP AF 300mm f/2.8 LD-IF (360E)"),
    ([0xE1,0x40,0x19,0x36,0x2C,0x35,0xDF,0x4E], "Tamron 10-24mm f/3.5-4.5 Di II VC HLD (B023)"),
    ([0xE1,0x40,0x19,0x36,0x2C,0x35,0xDF,0x0E], "Tamron 10-24mm f/3.5-4.5 Di II VC HLD (B023)"),
    ([0xF6,0x3F,0x18,0x37,0x2C,0x34,0x84,0x06], "Tamron SP AF 10-24mm f/3.5-4.5 Di II LD Aspherical (IF) (B001)"),
    ([0xF6,0x3F,0x18,0x37,0x2C,0x34,0xDF,0x06], "Tamron SP AF 10-24mm f/3.5-4.5 Di II LD Aspherical (IF) (B001)"),
    ([0x00,0x36,0x1C,0x2D,0x34,0x3C,0x00,0x06], "Tamron SP AF 11-18mm f/4.5-5.6 Di II LD Aspherical (IF) (A13)"),
    ([0xE9,0x48,0x27,0x3E,0x24,0x24,0xDF,0x0E], "Tamron SP 15-30mm f/2.8 Di VC USD (A012)"),
    ([0xCA,0x48,0x27,0x3E,0x24,0x24,0xDF,0x4E], "Tamron SP 15-30mm f/2.8 Di VC USD G2 (A041)"),
    ([0xEA,0x40,0x29,0x8E,0x2C,0x40,0xDF,0x0E], "Tamron 16-300mm f/3.5-6.3 Di II VC PZD (B016)"),
    ([0x07,0x46,0x2B,0x44,0x24,0x30,0x03,0x02], "Tamron SP AF 17-35mm f/2.8-4 Di LD Aspherical (IF) (A05)"),
    ([0xCB,0x3C,0x2B,0x44,0x24,0x31,0xDF,0x46], "Tamron 17-35mm f/2.8-4 Di OSD (A037)"),
    ([0x00,0x53,0x2B,0x50,0x24,0x24,0x00,0x06], "Tamron SP AF 17-50mm f/2.8 XR Di II LD Aspherical (IF) (A16)"),
    ([0x7C,0x54,0x2B,0x50,0x24,0x24,0x00,0x06], "Tamron SP AF 17-50mm f/2.8 XR Di II LD Aspherical (IF) (A16)"),
    ([0x00,0x54,0x2B,0x50,0x24,0x24,0x00,0x06], "Tamron SP AF 17-50mm f/2.8 XR Di II LD Aspherical (IF) (A16NII)"),
    ([0xFB,0x54,0x2B,0x50,0x24,0x24,0x84,0x06], "Tamron SP AF 17-50mm f/2.8 XR Di II LD Aspherical (IF) (A16NII)"),
    ([0xF3,0x54,0x2B,0x50,0x24,0x24,0x84,0x0E], "Tamron SP AF 17-50mm f/2.8 XR Di II VC LD Aspherical (IF) (B005)"),
    ([0x00,0x3F,0x2D,0x80,0x2B,0x40,0x00,0x06], "Tamron AF 18-200mm f/3.5-6.3 XR Di II LD Aspherical (IF) (A14)"),
    ([0x00,0x3F,0x2D,0x80,0x2C,0x40,0x00,0x06], "Tamron AF 18-200mm f/3.5-6.3 XR Di II LD Aspherical (IF) Macro (A14)"),
    ([0xEC,0x3E,0x3C,0x8E,0x2C,0x40,0xDF,0x0E], "Tamron 28-300mm f/3.5-6.3 Di VC PZD A010"),
    ([0x00,0x40,0x2D,0x80,0x2C,0x40,0x00,0x06], "Tamron AF 18-200mm f/3.5-6.3 XR Di II LD Aspherical (IF) Macro (A14NII)"),
    ([0xFC,0x40,0x2D,0x80,0x2C,0x40,0xDF,0x06], "Tamron AF 18-200mm f/3.5-6.3 XR Di II LD Aspherical (IF) Macro (A14NII)"),
    ([0xE6,0x40,0x2D,0x80,0x2C,0x40,0xDF,0x0E], "Tamron 18-200mm f/3.5-6.3 Di II VC (B018)"),
    ([0x00,0x40,0x2D,0x88,0x2C,0x40,0x62,0x06], "Tamron AF 18-250mm f/3.5-6.3 Di II LD Aspherical (IF) Macro (A18)"),
    ([0x00,0x40,0x2D,0x88,0x2C,0x40,0x00,0x06], "Tamron AF 18-250mm f/3.5-6.3 Di II LD Aspherical (IF) Macro (A18NII)"),
    ([0xF5,0x40,0x2C,0x8A,0x2C,0x40,0x40,0x0E], "Tamron AF 18-270mm f/3.5-6.3 Di II VC LD Aspherical (IF) Macro (B003)"),
    ([0xF0,0x3F,0x2D,0x8A,0x2C,0x40,0xDF,0x0E], "Tamron AF 18-270mm f/3.5-6.3 Di II VC PZD (B008)"),
    ([0xE0,0x40,0x2D,0x98,0x2C,0x41,0xDF,0x0E], "Tamron 18-400mm f/3.5-6.3 Di II VC HLD (B028)"),
    ([0xE0,0x40,0x2D,0x98,0x2C,0x41,0xDF,0x4E], "Tamron 18-400mm f/3.5-6.3 Di II VC HLD (B028)"),
    ([0x07,0x40,0x2F,0x44,0x2C,0x34,0x03,0x02], "Tamron AF 19-35mm f/3.5-4.5 (A10)"),
    ([0x00,0x49,0x30,0x48,0x22,0x2B,0x00,0x02], "Tamron SP AF 20-40mm f/2.7-3.5 (166D)"),
    ([0x0E,0x4A,0x31,0x48,0x23,0x2D,0x0E,0x02], "Tamron SP AF 20-40mm f/2.7-3.5 (166D)"),
    ([0xFE,0x48,0x37,0x5C,0x24,0x24,0xDF,0x0E], "Tamron SP 24-70mm f/2.8 Di VC USD (A007)"),
    ([0xCE,0x47,0x37,0x5C,0x25,0x25,0xDF,0x4E], "Tamron SP 24-70mm f/2.8 Di VC USD G2 (A032)"),
    ([0xCE,0x00,0x37,0x5C,0x25,0x25,0xDF,0x4E], "Tamron SP 24-70mm f/2.8 Di VC USD G2 (A032)"),
    ([0x45,0x41,0x37,0x72,0x2C,0x3C,0x48,0x02], "Tamron SP AF 24-135mm f/3.5-5.6 AD Aspherical (IF) Macro (190D)"),
    ([0x33,0x54,0x3C,0x5E,0x24,0x24,0x62,0x02], "Tamron SP AF 28-75mm f/2.8 XR Di LD Aspherical (IF) Macro (A09)"),
    ([0xFA,0x54,0x3C,0x5E,0x24,0x24,0x84,0x06], "Tamron SP AF 28-75mm f/2.8 XR Di LD Aspherical (IF) Macro (A09NII)"),
    ([0xFA,0x54,0x3C,0x5E,0x24,0x24,0xDF,0x06], "Tamron SP AF 28-75mm f/2.8 XR Di LD Aspherical (IF) Macro (A09NII)"),
    ([0x10,0x3D,0x3C,0x60,0x2C,0x3C,0xD2,0x02], "Tamron AF 28-80mm f/3.5-5.6 Aspherical (177D)"),
    ([0x45,0x3D,0x3C,0x60,0x2C,0x3C,0x48,0x02], "Tamron AF 28-80mm f/3.5-5.6 Aspherical (177D)"),
    ([0x00,0x48,0x3C,0x6A,0x24,0x24,0x00,0x02], "Tamron SP AF 28-105mm f/2.8 LD Aspherical IF (176D)"),
    ([0x4D,0x3E,0x3C,0x80,0x2E,0x3C,0x62,0x02], "Tamron AF 28-200mm f/3.8-5.6 XR Aspherical (IF) Macro (A03N)"),
    ([0x0B,0x3E,0x3D,0x7F,0x2F,0x3D,0x0E,0x00], "Tamron AF 28-200mm f/3.8-5.6 (71D)"),
    ([0x0B,0x3E,0x3D,0x7F,0x2F,0x3D,0x0E,0x02], "Tamron AF 28-200mm f/3.8-5.6D (171D)"),
    ([0x12,0x3D,0x3C,0x80,0x2E,0x3C,0xDF,0x02], "Tamron AF 28-200mm f/3.8-5.6 AF Aspherical LD (IF) (271D)"),
    ([0x4D,0x41,0x3C,0x8E,0x2B,0x40,0x62,0x02], "Tamron AF 28-300mm f/3.5-6.3 XR Di LD Aspherical (IF) (A061)"),
    ([0x4D,0x41,0x3C,0x8E,0x2C,0x40,0x62,0x02], "Tamron AF 28-300mm f/3.5-6.3 XR LD Aspherical (IF) (185D)"),
    ([0xF9,0x40,0x3C,0x8E,0x2C,0x40,0x40,0x0E], "Tamron AF 28-300mm f/3.5-6.3 XR Di VC LD Aspherical (IF) Macro (A20)"),
    ([0xC9,0x3C,0x44,0x76,0x25,0x31,0xDF,0x4E], "Tamron 35-150mm f/2.8-4 Di VC OSD (A043)"),
    ([0x00,0x47,0x53,0x80,0x30,0x3C,0x00,0x06], "Tamron AF 55-200mm f/4-5.6 Di II LD (A15)"),
    ([0xF7,0x53,0x5C,0x80,0x24,0x24,0x84,0x06], "Tamron SP AF 70-200mm f/2.8 Di LD (IF) Macro (A001)"),
    ([0xFE,0x53,0x5C,0x80,0x24,0x24,0x84,0x06], "Tamron SP AF 70-200mm f/2.8 Di LD (IF) Macro (A001)"),
    ([0xF7,0x53,0x5C,0x80,0x24,0x24,0x40,0x06], "Tamron SP AF 70-200mm f/2.8 Di LD (IF) Macro (A001)"),
    ([0xFE,0x54,0x5C,0x80,0x24,0x24,0xDF,0x0E], "Tamron SP 70-200mm f/2.8 Di VC USD (A009)"),
    ([0xE2,0x47,0x5C,0x80,0x24,0x24,0xDF,0x4E], "Tamron SP 70-200mm f/2.8 Di VC USD G2 (A025)"),
    ([0x69,0x48,0x5C,0x8E,0x30,0x3C,0x6F,0x02], "Tamron AF 70-300mm f/4-5.6 LD Macro 1:2 (572D/772D)"),
    ([0x69,0x47,0x5C,0x8E,0x30,0x3C,0x00,0x02], "Tamron AF 70-300mm f/4-5.6 Di LD Macro 1:2 (A17N)"),
    ([0x00,0x48,0x5C,0x8E,0x30,0x3C,0x00,0x06], "Tamron AF 70-300mm f/4-5.6 Di LD Macro 1:2 (A17NII)"),
    ([0xF1,0x47,0x5C,0x8E,0x30,0x3C,0xDF,0x0E], "Tamron SP 70-300mm f/4-5.6 Di VC USD (A005)"),
    ([0xCF,0x47,0x5C,0x8E,0x31,0x3D,0xDF,0x0E], "Tamron SP 70-300mm f/4-5.6 Di VC USD (A030)"),
    ([0xCC,0x44,0x68,0x98,0x34,0x41,0xDF,0x0E], "Tamron 100-400mm f/4.5-6.3 Di VC USD"),
    ([0xEB,0x40,0x76,0xA6,0x38,0x40,0xDF,0x0E], "Tamron SP AF 150-600mm f/5-6.3 VC USD (A011)"),
    ([0xE3,0x40,0x76,0xA6,0x38,0x40,0xDF,0x4E], "Tamron SP 150-600mm f/5-6.3 Di VC USD G2"),
    ([0xE3,0x40,0x76,0xA6,0x38,0x40,0xDF,0x0E], "Tamron SP 150-600mm f/5-6.3 Di VC USD G2 (A022)"),
    ([0x20,0x3C,0x80,0x98,0x3D,0x3D,0x1E,0x02], "Tamron AF 200-400mm f/5.6 LD IF (75D)"),
    ([0x00,0x3E,0x80,0xA0,0x38,0x3F,0x00,0x02], "Tamron SP AF 200-500mm f/5-6.3 Di LD (IF) (A08)"),
    ([0x00,0x3F,0x80,0xA0,0x38,0x3F,0x00,0x02], "Tamron SP AF 200-500mm f/5-6.3 Di (A08)"),
    ([0x00,0x40,0x2B,0x2B,0x2C,0x2C,0x00,0x02], "Tokina AT-X 17 AF PRO (AF 17mm f/3.5)"),
    ([0x00,0x47,0x44,0x44,0x24,0x24,0x00,0x06], "Tokina AT-X M35 PRO DX (AF 35mm f/2.8 Macro)"),
    ([0x8D,0x54,0x68,0x68,0x24,0x24,0x87,0x02], "Tokina AT-X PRO 100mm F2.8 D Macro"),
    ([0x00,0x54,0x68,0x68,0x24,0x24,0x00,0x02], "Tokina AT-X M100 AF PRO D (AF 100mm f/2.8 Macro)"),
    ([0x27,0x48,0x8E,0x8E,0x30,0x30,0x1D,0x02], "Tokina AT-X 304 AF (AF 300mm f/4.0)"),
    ([0x00,0x54,0x8E,0x8E,0x24,0x24,0x00,0x02], "Tokina AT-X 300 AF PRO (AF 300mm f/2.8)"),
    ([0x12,0x3B,0x98,0x98,0x3D,0x3D,0x09,0x00], "Tokina AT-X 400 AF SD (AF 400mm f/5.6)"),
    ([0x00,0x40,0x18,0x2B,0x2C,0x34,0x00,0x06], "Tokina AT-X 107 AF DX Fisheye (AF 10-17mm f/3.5-4.5)"),
    ([0x00,0x48,0x1C,0x29,0x24,0x24,0x00,0x06], "Tokina AT-X 116 PRO DX (AF 11-16mm f/2.8)"),
    ([0x7A,0x48,0x1C,0x29,0x24,0x24,0x7E,0x06], "Tokina AT-X 116 PRO DX II (AF 11-16mm f/2.8)"),
    ([0x80,0x48,0x1C,0x29,0x24,0x24,0x7A,0x06], "Tokina atx-i 11-16mm F2.8 CF"),
    ([0x7A,0x48,0x1C,0x30,0x24,0x24,0x7E,0x06], "Tokina AT-X 11-20 F2.8 PRO DX (AF 11-20mm f/2.8)"),
    ([0x8B,0x48,0x1C,0x30,0x24,0x24,0x85,0x06], "Tokina AT-X 11-20 F2.8 PRO DX (AF 11-20mm f/2.8)"),
    ([0x00,0x3C,0x1F,0x37,0x30,0x30,0x00,0x06], "Tokina AT-X 124 AF PRO DX (AF 12-24mm f/4)"),
    ([0x7A,0x3C,0x1F,0x3C,0x30,0x30,0x7E,0x06], "Tokina AT-X 12-28 PRO DX (AF 12-28mm f/4)"),
    ([0x00,0x48,0x29,0x3C,0x24,0x24,0x00,0x06], "Tokina AT-X 16-28 AF PRO FX (AF 16-28mm f/2.8)"),
    ([0x00,0x48,0x29,0x50,0x24,0x24,0x00,0x06], "Tokina AT-X 165 PRO DX (AF 16-50mm f/2.8)"),
    ([0x00,0x40,0x2A,0x72,0x2C,0x3C,0x00,0x06], "Tokina AT-X 16.5-135 DX (AF 16.5-135mm F3.5-5.6)"),
    ([0x00,0x3C,0x2B,0x44,0x30,0x30,0x00,0x06], "Tokina AT-X 17-35 F4 PRO FX (AF 17-35mm f/4)"),
    ([0x00,0x48,0x37,0x5C,0x24,0x24,0x00,0x06], "Tokina AT-X 24-70 F2.8 PRO FX (AF 24-70mm f/2.8)"),
    ([0x00,0x40,0x37,0x80,0x2C,0x3C,0x00,0x02], "Tokina AT-X 242 AF (AF 24-200mm f/3.5-5.6)"),
    ([0x07,0x48,0x3C,0x5C,0x24,0x24,0x03,0x00], "Tokina AT-X 287 AF (AF 28-70mm f/2.8)"),
    ([0x07,0x47,0x3C,0x5C,0x25,0x35,0x03,0x00], "Tokina AF 287 SD (AF 28-70mm f/2.8-4.5)"),
    ([0x07,0x40,0x3C,0x5C,0x2C,0x35,0x03,0x00], "Tokina AF 270 II (AF 28-70mm f/3.5-4.5)"),
    ([0x00,0x48,0x3C,0x60,0x24,0x24,0x00,0x02], "Tokina AT-X 280 AF PRO (AF 28-80mm f/2.8)"),
    ([0x25,0x44,0x44,0x8E,0x34,0x42,0x1B,0x02], "Tokina AF 353 (AF 35-300mm f/4.5-6.7)"),
    ([0x00,0x48,0x50,0x72,0x24,0x24,0x00,0x06], "Tokina AT-X 535 PRO DX (AF 50-135mm f/2.8)"),
    ([0x00,0x3C,0x5C,0x80,0x30,0x30,0x00,0x0E], "Tokina AT-X 70-200 F4 FX VCM-S (AF 70-200mm f/4)"),
    ([0x00,0x48,0x5C,0x80,0x30,0x30,0x00,0x0E], "Tokina AT-X 70-200 F4 FX VCM-S (AF 70-200mm f/4)"),
    ([0x12,0x44,0x5E,0x8E,0x34,0x3C,0x09,0x00], "Tokina AF 730 (AF 75-300mm F4.5-5.6)"),
    ([0x14,0x54,0x60,0x80,0x24,0x24,0x0B,0x00], "Tokina AT-X 828 AF (AF 80-200mm f/2.8)"),
    ([0x24,0x54,0x60,0x80,0x24,0x24,0x1A,0x02], "Tokina AT-X 828 AF PRO (AF 80-200mm f/2.8)"),
    ([0x24,0x44,0x60,0x98,0x34,0x3C,0x1A,0x02], "Tokina AT-X 840 AF-II (AF 80-400mm f/4.5-5.6)"),
    ([0x00,0x44,0x60,0x98,0x34,0x3C,0x00,0x02], "Tokina AT-X 840 D (AF 80-400mm f/4.5-5.6)"),
    ([0x14,0x48,0x68,0x8E,0x30,0x30,0x0B,0x00], "Tokina AT-X 340 AF (AF 100-300mm f/4)"),
    ([0x8C,0x48,0x29,0x3C,0x24,0x24,0x86,0x06], "Tokina opera 16-28mm F2.8 FF"),
    ([0x06,0x3F,0x68,0x68,0x2C,0x2C,0x06,0x00], "Cosina AF 100mm F3.5 Macro"),
    ([0x07,0x36,0x3D,0x5F,0x2C,0x3C,0x03,0x00], "Cosina AF Zoom 28-80mm F3.5-5.6 MC Macro"),
    ([0x07,0x46,0x3D,0x6A,0x25,0x2F,0x03,0x00], "Cosina AF Zoom 28-105mm F2.8-3.8 MC"),
    ([0x12,0x36,0x5C,0x81,0x35,0x3D,0x09,0x00], "Cosina AF Zoom 70-210mm F4.5-5.6 MC Macro"),
    ([0x12,0x39,0x5C,0x8E,0x34,0x3D,0x08,0x02], "Cosina AF Zoom 70-300mm F4.5-5.6 MC Macro"),
    ([0x12,0x3B,0x68,0x8D,0x3D,0x43,0x09,0x02], "Cosina AF Zoom 100-300mm F5.6-6.7 MC Macro"),
    ([0x12,0x38,0x69,0x97,0x35,0x42,0x09,0x02], "Promaster Spectrum 7 100-400mm F4.5-6.7"),
    ([0x00,0x40,0x31,0x31,0x2C,0x2C,0x00,0x00], "Voigtlander Color Skopar 20mm F3.5 SLII Aspherical"),
    ([0x00,0x48,0x3C,0x3C,0x24,0x24,0x00,0x00], "Voigtlander Color Skopar 28mm F2.8 SL II"),
    ([0x00,0x54,0x48,0x48,0x18,0x18,0x00,0x00], "Voigtlander Ultron 40mm F2 SLII Aspherical"),
    ([0x00,0x54,0x55,0x55,0x0C,0x0C,0x00,0x00], "Voigtlander Nokton 58mm F1.4 SLII"),
    ([0x00,0x40,0x64,0x64,0x2C,0x2C,0x00,0x00], "Voigtlander APO-Lanthar 90mm F3.5 SLII Close Focus"),
    ([0x71,0x48,0x64,0x64,0x24,0x24,0x00,0x00], "Voigtlander APO-Skopar 90mm F2.8 SL IIs"),
    ([0xFD,0x00,0x50,0x50,0x18,0x18,0xDF,0x00], "Voigtlander APO-Lanthar 50mm F2 Aspherical"),
    ([0xFD,0x00,0x44,0x44,0x18,0x18,0xDF,0x00], "Voigtlander APO-Lanthar 35mm F2"),
    ([0xFD,0x00,0x59,0x59,0x18,0x18,0xDF,0x00], "Voigtlander Macro APO-Lanthar 65mm F2"),
    ([0xFD,0x00,0x48,0x48,0x07,0x07,0xDF,0x00], "Voigtlander Nokton 40mm F1.2 Aspherical"),
    ([0xFD,0x00,0x3C,0x3C,0x18,0x18,0xDF,0x00], "Voigtlander APO-Lanthar 28mm F2 Aspherical"),
    ([0x00,0x40,0x2D,0x2D,0x2C,0x2C,0x00,0x00], "Carl Zeiss Distagon T* 3.5/18 ZF.2"),
    ([0x00,0x48,0x27,0x27,0x24,0x24,0x00,0x00], "Carl Zeiss Distagon T* 2.8/15 ZF.2"),
    ([0x00,0x48,0x32,0x32,0x24,0x24,0x00,0x00], "Carl Zeiss Distagon T* 2.8/21 ZF.2"),
    ([0x00,0x54,0x38,0x38,0x18,0x18,0x00,0x00], "Carl Zeiss Distagon T* 2/25 ZF.2"),
    ([0x00,0x54,0x3C,0x3C,0x18,0x18,0x00,0x00], "Carl Zeiss Distagon T* 2/28 ZF.2"),
    ([0x00,0x54,0x44,0x44,0x0C,0x0C,0x00,0x00], "Carl Zeiss Distagon T* 1.4/35 ZF.2"),
    ([0x00,0x54,0x44,0x44,0x18,0x18,0x00,0x00], "Carl Zeiss Distagon T* 2/35 ZF.2"),
    ([0x00,0x54,0x50,0x50,0x0C,0x0C,0x00,0x00], "Carl Zeiss Planar T* 1.4/50 ZF.2"),
    ([0x00,0x54,0x50,0x50,0x18,0x18,0x00,0x00], "Carl Zeiss Makro-Planar T* 2/50 ZF.2"),
    ([0x00,0x54,0x62,0x62,0x0C,0x0C,0x00,0x00], "Carl Zeiss Planar T* 1.4/85 ZF.2"),
    ([0x00,0x54,0x68,0x68,0x18,0x18,0x00,0x00], "Carl Zeiss Makro-Planar T* 2/100 ZF.2"),
    ([0x00,0x54,0x72,0x72,0x18,0x18,0x00,0x00], "Carl Zeiss Apo Sonnar T* 2/135 ZF.2"),
    ([0x02,0x54,0x3C,0x3C,0x0C,0x0C,0x00,0x00], "Zeiss Otus 1.4/28 ZF.2"),
    ([0x00,0x54,0x53,0x53,0x0C,0x0C,0x00,0x00], "Zeiss Otus 1.4/55"),
    ([0x01,0x54,0x62,0x62,0x0C,0x0C,0x00,0x00], "Zeiss Otus 1.4/85"),
    ([0x03,0x54,0x68,0x68,0x0C,0x0C,0x00,0x00], "Zeiss Otus 1.4/100"),
    ([0x52,0x54,0x44,0x44,0x18,0x18,0x00,0x00], "Zeiss Milvus 35mm f/2"),
    ([0x53,0x54,0x50,0x50,0x0C,0x0C,0x00,0x00], "Zeiss Milvus 50mm f/1.4"),
    ([0x54,0x54,0x50,0x50,0x18,0x18,0x00,0x00], "Zeiss Milvus 50mm f/2 Macro"),
    ([0x55,0x54,0x62,0x62,0x0C,0x0C,0x00,0x00], "Zeiss Milvus 85mm f/1.4"),
    ([0x56,0x54,0x68,0x68,0x18,0x18,0x00,0x00], "Zeiss Milvus 100mm f/2 Macro"),
    ([0x00,0x54,0x56,0x56,0x30,0x30,0x00,0x00], "Coastal Optical Systems 60mm 1:4 UV-VIS-IR Macro Apo"),
    ([0xBF,0x4E,0x26,0x26,0x1E,0x1E,0x01,0x04], "Irix 15mm f/2.4 Firefly"),
    ([0xBF,0x3C,0x1B,0x1B,0x30,0x30,0x01,0x04], "Irix 11mm f/4 Firefly"),
    ([0x4A,0x40,0x11,0x11,0x2C,0x0C,0x4D,0x02], "Samyang 8mm f/3.5 Fish-Eye CS"),
    ([0x4A,0x48,0x1E,0x1E,0x24,0x0C,0x4D,0x02], "Samyang 12mm f/2.8 ED AS NCS Fish-Eye"),
    ([0x4A,0x4C,0x24,0x24,0x1E,0x6C,0x4D,0x06], "Samyang 14mm f/2.4 Premium"),
    ([0x4A,0x54,0x29,0x29,0x18,0x0C,0x4D,0x02], "Samyang 16mm f/2.0 ED AS UMC CS"),
    ([0x4A,0x60,0x36,0x36,0x0C,0x0C,0x4D,0x02], "Samyang 24mm f/1.4 ED AS UMC"),
    ([0x4A,0x60,0x44,0x44,0x0C,0x0C,0x4D,0x02], "Samyang 35mm f/1.4 AS UMC"),
    ([0x4A,0x60,0x62,0x62,0x0C,0x0C,0x4D,0x02], "Samyang AE 85mm f/1.4 AS IF UMC"),
    ([0x9A,0x4C,0x50,0x50,0x14,0x14,0x9C,0x06], "Yongnuo YN50mm F1.8N"),
    ([0x9F,0x48,0x48,0x48,0x24,0x24,0xA1,0x06], "Yongnuo YN40mm F2.8N"),
    ([0x9F,0x54,0x68,0x68,0x18,0x18,0xA2,0x06], "Yongnuo YN100mm F2N"),
    ([0x9F,0x4C,0x44,0x44,0x18,0x18,0xA1,0x06], "Yongnuo YN35mm F2"),
    ([0x9F,0x4D,0x50,0x50,0x14,0x14,0xA0,0x06], "Yongnuo YN50mm F1.8N"),
    ([0x02,0x40,0x44,0x5C,0x2C,0x34,0x02,0x00], "Exakta AF 35-70mm 1:3.5-4.5 MC"),
    ([0x07,0x3E,0x30,0x43,0x2D,0x35,0x03,0x00], "Soligor AF Zoom 19-35mm 1:3.5-4.5 MC"),
    ([0x03,0x43,0x5C,0x81,0x35,0x35,0x02,0x00], "Soligor AF C/D Zoom UMCS 70-210mm 1:4.5"),
    ([0x12,0x4A,0x5C,0x81,0x31,0x3D,0x09,0x00], "Soligor AF C/D Auto Zoom+Macro 70-210mm 1:4-5.6 UMCS"),
    ([0x12,0x36,0x69,0x97,0x35,0x42,0x09,0x00], "Soligor AF Zoom 100-400mm 1:4.5-6.7 MC"),
    ([0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x01], "Manual Lens No CPU"),
    ([0x00,0x00,0x48,0x48,0x53,0x53,0x00,0x01], "Loreo 40mm F11-22 3D Lens in a Cap 9005"),
    ([0x00,0x47,0x10,0x10,0x24,0x24,0x00,0x00], "Fisheye Nikkor 8mm f/2.8 AiS"),
    ([0x00,0x47,0x3C,0x3C,0x24,0x24,0x00,0x00], "Nikkor 28mm f/2.8 AiS"),
    ([0x00,0x57,0x50,0x50,0x14,0x14,0x00,0x00], "Nikkor 50mm f/1.8 AI"),
    ([0x00,0x48,0x50,0x50,0x18,0x18,0x00,0x00], "Nikkor H 50mm f/2"),
    ([0x00,0x48,0x68,0x68,0x24,0x24,0x00,0x00], "Series E 100mm f/2.8"),
    ([0x00,0x4C,0x6A,0x6A,0x20,0x20,0x00,0x00], "Nikkor 105mm f/2.5 AiS"),
    ([0x00,0x48,0x80,0x80,0x30,0x30,0x00,0x00], "Nikkor 200mm f/4 AiS"),
    ([0x00,0x40,0x11,0x11,0x2C,0x2C,0x00,0x00], "Samyang 8mm f/3.5 Fish-Eye"),
    ([0x00,0x58,0x64,0x64,0x20,0x20,0x00,0x00], "Soligor C/D Macro MC 90mm f/2.5"),
    ([0x4A,0x58,0x30,0x30,0x14,0x0C,0x4D,0x02], "Rokinon 20mm f/1.8 ED AS UMC"),
    ([0xA0,0x56,0x44,0x44,0x14,0x14,0xA2,0x06], "Sony FE 35mm F1.8"),
    ([0xA0,0x37,0x5C,0x8E,0x34,0x3C,0xA2,0x06], "Sony FE 70-300mm F4.5-5.6 G OSS"),
];
