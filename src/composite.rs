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

    // LensID / Lens: combine LensMake + LensModel or LensInfo
    if let Some(lens) = compute_lens(tags) {
        composite.push(lens);
    }

    // ShutterSpeed: from ExposureTime
    if let Some(ss) = compute_shutter_speed(tags) {
        composite.push(ss);
    }

    // Aperture: from FNumber
    if let Some(ap) = compute_aperture(tags) {
        composite.push(ap);
    }

    // ImageSize: Width x Height
    if let Some(sz) = compute_image_size(tags) {
        composite.push(sz);
    }

    // Megapixels
    if let Some(mp) = compute_megapixels(tags) {
        composite.push(mp);
    }

    // LightValue
    if let Some(lv) = compute_light_value(tags) {
        composite.push(lv);
    }

    // DateTimeCreated: combine DateTimeOriginal + SubSecTimeOriginal + OffsetTimeOriginal
    if let Some(dt) = compute_datetime_created(tags) {
        composite.push(dt);
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
    }

    // RedBalance + BlueBalance
    if let Some(wb_tags) = compute_wb_balance(tags) {
        composite.extend(wb_tags);
    }

    // DOF (Depth of Field)
    if let Some(dof_tags) = compute_dof(tags) {
        composite.extend(dof_tags);
    }

    // FocalPlaneXSize / FocalPlaneYSize
    if let Some(fp_tags) = compute_focal_plane_size(tags) {
        composite.extend(fp_tags);
    }

    // GPSDateTime composite
    if let (Some(date), Some(time)) = (find_tag_value(tags, "GPSDateStamp"), find_tag_value(tags, "GPSTimeStamp")) {
        if !date.is_empty() && !time.is_empty() {
            composite.push(mk_composite("GPSDateTime", "GPS Date/Time",
                Value::String(format!("{} {}Z", date, time))));
        }
    }

    // Nikon SerialNumber (from SerialNumber2 or InternalSerialNumber)
    if find_tag(tags, "SerialNumber").is_none() {
        if let Some(sn) = find_tag_value(tags, "SerialNumber2")
            .or_else(|| find_tag_value(tags, "InternalSerialNumber"))
        {
            composite.push(mk_composite("SerialNumber", "Serial Number", Value::String(sn)));
        }
    }

    // LensSpec composite
    if find_tag(tags, "LensSpec").is_none() {
        let min_fl = find_tag_f64(tags, "MinFocalLength");
        let max_fl = find_tag_f64(tags, "MaxFocalLength");
        let min_ap = find_tag_value(tags, "MaxApertureAtMinFocal");
        let max_ap = find_tag_value(tags, "MaxApertureAtMaxFocal");
        if let (Some(min), Some(max)) = (min_fl, max_fl) {
            if min > 0.0 && max > 0.0 {
                let spec = if let (Some(ap_min), Some(ap_max)) = (min_ap, max_ap) {
                    format!("{:.0}-{:.0}mm f/{}-{}", min, max, ap_min, ap_max)
                } else {
                    format!("{:.0}-{:.0}mm", min, max)
                };
                composite.push(mk_composite("LensSpec", "Lens Spec", Value::String(spec)));
            }
        }
    }

    // AutoFocus (from AFInfo)
    if let Some(afm) = find_tag_value(tags, "AFAreaMode") {
        if find_tag(tags, "AutoFocus").is_none() {
            let af = if afm.contains("Manual") { "Off" } else { "On" };
            composite.push(mk_composite("AutoFocus", "Auto Focus", Value::String(af.into())));
        }
    }

    // Canon-specific composites
    if let Some(canon_tags) = compute_canon_composites(tags) {
        composite.extend(canon_tags);
    }

    // FOV (Field of View) - only if not already added by compute_35efl
    if !composite.iter().any(|t| t.name == "FOV") {
        if let Some(fov) = compute_fov(tags) {
            composite.push(fov);
        }
    }

    // HyperfocalDistance
    if let Some(hd) = compute_hyperfocal(tags) {
        composite.push(hd);
    }

    // CircleOfConfusion
    if let Some(coc) = compute_circle_of_confusion(tags) {
        composite.push(coc);
    }

    composite
}

fn find_tag<'a>(tags: &'a [Tag], name: &str) -> Option<&'a Tag> {
    let name_lower = name.to_lowercase();
    tags.iter().find(|t| t.name.to_lowercase() == name_lower)
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
            let deg = items[0].as_f64().unwrap_or(0.0);
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

fn compute_lens(tags: &[Tag]) -> Option<Tag> {
    // Try LensModel first
    if let Some(model) = find_tag_value(tags, "LensModel") {
        if !model.is_empty() {
            return Some(mk_composite("Lens", "Lens", Value::String(model)));
        }
    }

    // Try LensInfo (min focal, max focal, min aperture, max aperture)
    if let Some(info) = find_tag(tags, "LensInfo") {
        return Some(mk_composite(
            "Lens",
            "Lens",
            Value::String(info.print_value.clone()),
        ));
    }

    // Try combining FocalLength
    if let Some(fl) = find_tag_value(tags, "FocalLength") {
        return Some(mk_composite("Lens", "Lens", Value::String(fl)));
    }

    None
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

fn compute_aperture(tags: &[Tag]) -> Option<Tag> {
    let fnum = find_tag(tags, "FNumber")?;
    Some(mk_composite(
        "Aperture",
        "Aperture",
        Value::String(fnum.print_value.clone()),
    ))
}

fn compute_image_size(tags: &[Tag]) -> Option<Tag> {
    // Prefer ExifImage dimensions (JPEG SOF), then EXIF tags, then TIFF IFD
    let w = find_tag(tags, "ExifImageWidth")
        .or_else(|| find_tag(tags, "ImageWidth"))?;
    let h = find_tag(tags, "ExifImageHeight")
        .or_else(|| find_tag(tags, "ImageHeight"))?;

    let width = w.raw_value.as_u64().or_else(|| w.print_value.parse().ok())?;
    let height = h.raw_value.as_u64().or_else(|| h.print_value.parse().ok())?;

    Some(mk_composite(
        "ImageSize",
        "Image Size",
        Value::String(format!("{}x{}", width, height)),
    ))
}

fn compute_megapixels(tags: &[Tag]) -> Option<Tag> {
    let w = find_tag(tags, "ExifImageWidth")
        .or_else(|| find_tag(tags, "ImageWidth"))?;
    let h = find_tag(tags, "ExifImageHeight")
        .or_else(|| find_tag(tags, "ExifImageHeight"))?;

    let width = w.raw_value.as_u64().or_else(|| w.print_value.parse().ok())?;
    let height = h.raw_value.as_u64().or_else(|| h.print_value.parse().ok())?;

    let mp = (width * height) as f64 / 1_000_000.0;
    if mp < 0.001 {
        return None;
    }

    Some(mk_composite(
        "Megapixels",
        "Megapixels",
        Value::String(format!("{:.1}", mp)),
    ))
}

fn compute_light_value(tags: &[Tag]) -> Option<Tag> {
    let aperture = find_tag_f64(tags, "FNumber")?;
    let exposure = find_tag_f64(tags, "ExposureTime")?;
    let iso = find_tag_f64(tags, "ISO")?;

    if exposure <= 0.0 || iso <= 0.0 || aperture <= 0.0 {
        return None;
    }

    // LV = log2(aperture^2 / exposure) - log2(iso / 100)
    let lv = (aperture * aperture / exposure).log2() - (iso / 100.0).log2();

    Some(mk_composite(
        "LightValue",
        "Light Value",
        Value::String(format!("{:.1}", lv)),
    ))
}

fn compute_datetime_created(tags: &[Tag]) -> Option<Tag> {
    let dt = find_tag_value(tags, "DateTimeOriginal")?;
    let subsec = find_tag_value(tags, "SubSecTimeOriginal").unwrap_or_default();
    let offset = find_tag_value(tags, "OffsetTimeOriginal").unwrap_or_default();

    let mut result = dt;
    if !subsec.is_empty() {
        result = format!("{}.{}", result, subsec);
    }
    if !offset.is_empty() {
        result = format!("{}{}", result, offset);
    }

    Some(mk_composite(
        "DateTimeCreated",
        "Date/Time Created",
        Value::String(result),
    ))
}

/// Compute 35mm equivalent focal length and scale factor.
fn compute_35efl(tags: &[Tag]) -> Option<Vec<Tag>> {
    let fl = find_tag_f64(tags, "FocalLength")?;
    if fl <= 0.0 { return None; }

    let mut result = Vec::new();

    // Compute scale factor
    let scale = if let Some(fl35) = find_tag_f64(tags, "FocalLengthIn35mmFormat") {
        if fl35 > 0.0 { fl35 / fl } else { return None; }
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
        if sensor_diag <= 0.0 { return None; }
        43.2666 / sensor_diag
    };

    let fl35_val = fl * scale;

    result.push(mk_composite("ScaleFactor35efl", "Scale Factor To 35 mm Equivalent",
        Value::String(format!("{:.1}", scale))));
    result.push(mk_composite("FocalLength35efl", "Focal Length (35mm equivalent)",
        Value::String(format!("{:.1} mm (35 mm equivalent: {:.1} mm)", fl, fl35_val))));

    // CircleOfConfusion
    let coc = 43.27 / scale / 1500.0;
    result.push(mk_composite("CircleOfConfusion", "Circle of Confusion",
        Value::String(format!("{:.3} mm", coc))));

    // FOV
    let fov = 2.0 * (36.0 / (2.0 * fl35_val)).atan() * 180.0 / std::f64::consts::PI;
    result.push(mk_composite("FOV", "Field of View",
        Value::String(format!("{:.1} deg", fov))));

    // Lens + Lens35efl
    let min_fl = find_tag_f64(tags, "MinFocalLength");
    let max_fl = find_tag_f64(tags, "MaxFocalLength");
    if let (Some(min), Some(max)) = (min_fl, max_fl) {
        if min > 0.0 && max > 0.0 && max > min {
            result.push(mk_composite("Lens", "Lens",
                Value::String(format!("{:.1} - {:.1} mm", min, max))));
            result.push(mk_composite("Lens35efl", "Lens (35mm equivalent)",
                Value::String(format!("{:.1} - {:.1} mm (35 mm equivalent: {:.1} - {:.1} mm)",
                    min, max, min * scale, max * scale))));
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
fn make_subsec_date(tags: &[Tag], date_tag: &str, subsec_tag: &str, offset_tag: &str, output_name: &str) -> Option<Tag> {
    let dt = find_tag_value(tags, date_tag)?;
    if dt.is_empty() { return None; }

    let subsec = find_tag_value(tags, subsec_tag).unwrap_or_default();
    let offset = find_tag_value(tags, offset_tag).unwrap_or_default();

    let mut result = dt;
    if !subsec.is_empty() {
        result = format!("{}.{}", result, subsec.trim());
    }
    if !offset.is_empty() {
        result = format!("{}{}", result, offset.trim());
    }

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
        }
    }

    if result.is_empty() { None } else { Some(result) }
}

/// Compute Depth of Field.
fn compute_dof(tags: &[Tag]) -> Option<Vec<Tag>> {
    let fl = find_tag_f64(tags, "FocalLength")?;
    let fnum = find_tag_f64(tags, "FNumber")?;
    let dist_upper = find_tag_f64(tags, "FocusDistanceUpper");
    let dist_lower = find_tag_f64(tags, "FocusDistanceLower");

    // Use average of upper/lower focus distance
    let distance = match (dist_upper, dist_lower) {
        (Some(u), Some(l)) if u > 0.0 && l > 0.0 => (u + l) / 2.0,
        (Some(u), _) if u > 0.0 => u,
        (_, Some(l)) if l > 0.0 => l,
        _ => return None,
    };

    if fl <= 0.0 || fnum <= 0.0 || distance <= 0.0 { return None; }

    // Circle of confusion (assume 35mm equivalent)
    let fl35 = find_tag_f64(tags, "FocalLengthIn35mmFormat").unwrap_or(fl * 1.6);
    let crop = fl35 / fl;
    let coc = 43.27 / crop / 1500.0; // mm

    // Hyperfocal distance
    let h = (fl * fl) / (fnum * coc) + fl; // mm
    let _h_m = h / 1000.0; // meters

    // DOF near and far limits (in meters)
    let dist_mm = distance * 1000.0;
    let near = dist_mm * (h - fl) / (h + dist_mm - 2.0 * fl);
    let far = if dist_mm < h {
        dist_mm * (h - fl) / (h - dist_mm)
    } else {
        f64::INFINITY
    };

    let dof = if far.is_infinite() {
        "inf".to_string()
    } else {
        format!("{:.2} m", (far - near) / 1000.0)
    };

    Some(vec![mk_composite("DOF", "Depth of Field", Value::String(dof))])
}

/// Compute focal plane physical size.
fn compute_focal_plane_size(tags: &[Tag]) -> Option<Vec<Tag>> {
    let xres = find_tag_f64(tags, "FocalPlaneXResolution")?;
    let yres = find_tag_f64(tags, "FocalPlaneYResolution")?;
    let width = find_tag_f64(tags, "ExifImageWidth")
        .or_else(|| find_tag_f64(tags, "ImageWidth"))?;
    let height = find_tag_f64(tags, "ExifImageHeight")
        .or_else(|| find_tag_f64(tags, "ImageHeight"))?;

    if xres <= 0.0 || yres <= 0.0 { return None; }

    let unit = find_tag_f64(tags, "FocalPlaneResolutionUnit").unwrap_or(2.0);
    let factor = match unit as u32 {
        2 => 25.4,   // inches to mm
        3 => 10.0,   // cm to mm
        _ => 25.4,   // default inches
    };

    let x_size = width * factor / xres;
    let y_size = height * factor / yres;

    let mut result = Vec::new();
    result.push(mk_composite("FocalPlaneXSize", "Focal Plane X Size",
        Value::String(format!("{:.2} mm", x_size))));
    result.push(mk_composite("FocalPlaneYSize", "Focal Plane Y Size",
        Value::String(format!("{:.2} mm", y_size))));
    Some(result)
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

/// Compute Field of View from FocalLength and sensor size.
fn compute_fov(tags: &[Tag]) -> Option<Tag> {
    let _fl = find_tag_f64(tags, "FocalLength")?;
    let fl35 = find_tag_f64(tags, "FocalLengthIn35mmFormat");

    if let Some(fl35) = fl35 {
        if fl35 > 0.0 {
            let fov = 2.0 * (36.0 / (2.0 * fl35)).atan() * 180.0 / std::f64::consts::PI;
            return Some(mk_composite("FOV", "Field of View", Value::String(format!("{:.1} deg", fov))));
        }
    }
    None
}

/// Compute Hyperfocal Distance: H = f² / (N × c)
fn compute_hyperfocal(tags: &[Tag]) -> Option<Tag> {
    let fl = find_tag_f64(tags, "FocalLength")?;
    let fnum = find_tag_f64(tags, "FNumber")?;

    if fl <= 0.0 || fnum <= 0.0 {
        return None;
    }

    // Assume 35mm full-frame circle of confusion = 0.030mm
    let coc = 0.030;
    let h_mm = (fl * fl) / (fnum * coc) + fl;
    let h_m = h_mm / 1000.0;

    Some(mk_composite(
        "HyperfocalDistance",
        "Hyperfocal Distance",
        Value::String(format!("{:.2} m", h_m)),
    ))
}

/// Approximate circle of confusion based on sensor size.
fn compute_circle_of_confusion(tags: &[Tag]) -> Option<Tag> {
    let fl = find_tag_f64(tags, "FocalLength")?;
    let fl35 = find_tag_f64(tags, "FocalLengthIn35mmFormat")?;

    if fl <= 0.0 || fl35 <= 0.0 {
        return None;
    }

    let crop_factor = fl35 / fl;
    let diagonal = 43.27; // 35mm diagonal in mm
    let sensor_diag = diagonal / crop_factor;
    let coc = sensor_diag / 1500.0; // Standard formula

    Some(mk_composite(
        "CircleOfConfusion",
        "Circle of Confusion",
        Value::String(format!("{:.3} mm", coc)),
    ))
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
