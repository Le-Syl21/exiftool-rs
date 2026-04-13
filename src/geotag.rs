//! GPX file parsing and GPS geotagging support.
//!
//! Implements the `-geotag` option: reads GPS track points from a GPX file
//! and writes GPS coordinates to images based on their DateTimeOriginal timestamps.

/// A single GPS track point from a GPX file.
#[derive(Debug, Clone)]
pub struct GpxPoint {
    pub lat: f64,
    pub lon: f64,
    pub ele: f64,
    pub time: i64, // Unix timestamp
}

/// GPS coordinates interpolated for a specific time.
#[derive(Debug, Clone)]
pub struct GpsCoords {
    pub lat: f64,
    pub lon: f64,
    pub ele: f64,
}

/// Parse a GPX file and extract track points.
///
/// GPX is XML with `<trkpt>` elements containing lat/lon attributes
/// and optional `<ele>` and `<time>` child elements.
pub fn parse_gpx(data: &str) -> Vec<GpxPoint> {
    let mut points = Vec::new();
    let mut pos = 0;
    let bytes = data.as_bytes();

    while pos < bytes.len() {
        // Find next <trkpt
        let trkpt_start = match find_str(data, pos, "<trkpt") {
            Some(p) => p,
            None => break,
        };

        // Find the closing > of the trkpt opening tag or />
        let tag_content_start = trkpt_start + 6; // skip "<trkpt"

        // Find the end of this trkpt element (either /> or </trkpt>)
        let trkpt_end = if let Some(close) = find_str(data, trkpt_start, "</trkpt>") {
            close + 8
        } else if let Some(close) = find_str(data, tag_content_start, "/>") {
            close + 2
        } else {
            pos = tag_content_start;
            continue;
        };

        let segment = &data[trkpt_start..trkpt_end];

        // Extract lat and lon attributes
        let lat = extract_attribute(segment, "lat");
        let lon = extract_attribute(segment, "lon");

        if let (Some(lat_val), Some(lon_val)) = (lat, lon) {
            let ele = extract_element_value(segment, "ele")
                .and_then(|s| s.parse::<f64>().ok())
                .unwrap_or(0.0);

            let time = extract_element_value(segment, "time").and_then(|s| parse_iso8601(&s));

            if let Some(timestamp) = time {
                points.push(GpxPoint {
                    lat: lat_val,
                    lon: lon_val,
                    ele,
                    time: timestamp,
                });
            }
        }

        pos = trkpt_end;
    }

    // Sort by time
    points.sort_by_key(|p| p.time);
    points
}

/// Find the GPS coordinates for a given Unix timestamp by interpolating
/// between the two closest track points.
pub fn find_gps_for_time(points: &[GpxPoint], timestamp: i64) -> Option<GpsCoords> {
    if points.is_empty() {
        return None;
    }

    // If only one point, return it
    if points.len() == 1 {
        return Some(GpsCoords {
            lat: points[0].lat,
            lon: points[0].lon,
            ele: points[0].ele,
        });
    }

    // If before first point or after last point, use the nearest
    if timestamp <= points[0].time {
        return Some(GpsCoords {
            lat: points[0].lat,
            lon: points[0].lon,
            ele: points[0].ele,
        });
    }
    if timestamp >= points[points.len() - 1].time {
        let last = &points[points.len() - 1];
        return Some(GpsCoords {
            lat: last.lat,
            lon: last.lon,
            ele: last.ele,
        });
    }

    // Binary search for the surrounding points
    let idx = match points.binary_search_by_key(&timestamp, |p| p.time) {
        Ok(i) => {
            // Exact match
            return Some(GpsCoords {
                lat: points[i].lat,
                lon: points[i].lon,
                ele: points[i].ele,
            });
        }
        Err(i) => i, // insertion point: points[i-1].time < timestamp < points[i].time
    };

    let p1 = &points[idx - 1];
    let p2 = &points[idx];

    // Linear interpolation
    let total = (p2.time - p1.time) as f64;
    if total == 0.0 {
        return Some(GpsCoords {
            lat: p1.lat,
            lon: p1.lon,
            ele: p1.ele,
        });
    }

    let frac = (timestamp - p1.time) as f64 / total;
    Some(GpsCoords {
        lat: p1.lat + (p2.lat - p1.lat) * frac,
        lon: p1.lon + (p2.lon - p1.lon) * frac,
        ele: p1.ele + (p2.ele - p1.ele) * frac,
    })
}

/// Parse an ExifTool-format date string (`YYYY:MM:DD HH:MM:SS`) to a Unix timestamp.
pub fn parse_exif_datetime(dt: &str) -> Option<i64> {
    let dt = dt.trim();
    if dt.len() < 19 {
        return None;
    }

    let year: i64 = dt[0..4].parse().ok()?;
    let month: i64 = dt[5..7].parse().ok()?;
    let day: i64 = dt[8..10].parse().ok()?;
    let hour: i64 = dt[11..13].parse().ok()?;
    let min: i64 = dt[14..16].parse().ok()?;
    let sec: i64 = dt[17..19].parse().ok()?;

    if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }

    Some(datetime_to_unix(year, month, day, hour, min, sec))
}

// ============================================================================
// Internal helpers
// ============================================================================

/// Find a substring starting at `from`.
fn find_str(data: &str, from: usize, needle: &str) -> Option<usize> {
    data[from..].find(needle).map(|i| from + i)
}

/// Extract an XML attribute value, e.g. `lat="46.123"` -> Some(46.123).
fn extract_attribute(segment: &str, attr: &str) -> Option<f64> {
    let pattern = format!("{}=\"", attr);
    let start = segment.find(&pattern)? + pattern.len();
    let end = segment[start..].find('"')? + start;
    segment[start..end].parse().ok()
}

/// Extract the text content of a child XML element, e.g. `<ele>449.0</ele>` -> Some("449.0").
fn extract_element_value(segment: &str, tag: &str) -> Option<String> {
    let open = format!("<{}>", tag);
    let close = format!("</{}>", tag);
    // Also handle lowercase/mixed case in GPX files
    let start = segment.find(&open).or_else(|| {
        // Try case-insensitive
        let lower = segment.to_lowercase();
        lower.find(&open.to_lowercase())
    })?;
    let content_start = start + open.len();
    let end = segment[content_start..].find(&close).or_else(|| {
        let lower = segment[content_start..].to_lowercase();
        lower.find(&close.to_lowercase())
    })?;
    Some(
        segment[content_start..content_start + end]
            .trim()
            .to_string(),
    )
}

/// Parse an ISO 8601 datetime string to a Unix timestamp.
/// Supports formats like `2024-01-15T10:30:00Z` and `2024-01-15T10:30:00+01:00`.
fn parse_iso8601(s: &str) -> Option<i64> {
    let s = s.trim();
    if s.len() < 19 {
        return None;
    }

    let year: i64 = s[0..4].parse().ok()?;
    let month: i64 = s[5..7].parse().ok()?;
    let day: i64 = s[8..10].parse().ok()?;
    let hour: i64 = s[11..13].parse().ok()?;
    let min: i64 = s[14..16].parse().ok()?;
    let sec: i64 = s[17..19].parse().ok()?;

    let mut ts = datetime_to_unix(year, month, day, hour, min, sec);

    // Handle timezone offset
    let rest = &s[19..];
    if rest.starts_with('Z') || rest.is_empty() {
        // Already UTC
    } else if rest.starts_with('+') || rest.starts_with('-') {
        let sign: i64 = if rest.starts_with('+') { -1 } else { 1 };
        let tz = &rest[1..];
        let (tz_h, tz_m) = if tz.contains(':') {
            let parts: Vec<&str> = tz.split(':').collect();
            (
                parts[0].parse::<i64>().unwrap_or(0),
                parts
                    .get(1)
                    .and_then(|p| p.parse::<i64>().ok())
                    .unwrap_or(0),
            )
        } else if tz.len() >= 4 {
            (
                tz[0..2].parse::<i64>().unwrap_or(0),
                tz[2..4].parse::<i64>().unwrap_or(0),
            )
        } else {
            (tz.parse::<i64>().unwrap_or(0), 0)
        };
        ts += sign * (tz_h * 3600 + tz_m * 60);
    }

    Some(ts)
}

/// Convert a date/time to Unix timestamp (seconds since 1970-01-01T00:00:00Z).
/// Simplified calculation assuming Gregorian calendar.
fn datetime_to_unix(year: i64, month: i64, day: i64, hour: i64, min: i64, sec: i64) -> i64 {
    // Days from year 0 to the start of the given year
    let y = if month <= 2 { year - 1 } else { year };
    let m = if month <= 2 { month + 9 } else { month - 3 };

    // Days since March 1, year 0 (using the Rata Die algorithm)
    let days = 365 * y + y / 4 - y / 100 + y / 400 + (m * 306 + 5) / 10 + (day - 1);

    // Unix epoch is 1970-01-01 = day 719468 in this system
    let unix_days = days - 719468;

    unix_days * 86400 + hour * 3600 + min * 60 + sec
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_gpx() {
        let gpx = r#"<?xml version="1.0" encoding="UTF-8"?>
<gpx version="1.1">
  <trk>
    <trkseg>
      <trkpt lat="46.57608" lon="6.62233">
        <ele>449.0</ele>
        <time>2024-01-15T10:30:00Z</time>
      </trkpt>
      <trkpt lat="46.57700" lon="6.62300">
        <ele>450.0</ele>
        <time>2024-01-15T10:31:00Z</time>
      </trkpt>
    </trkseg>
  </trk>
</gpx>"#;
        let points = parse_gpx(gpx);
        assert_eq!(points.len(), 2);
        assert!((points[0].lat - 46.57608).abs() < 1e-5);
        assert!((points[0].lon - 6.62233).abs() < 1e-5);
        assert!((points[0].ele - 449.0).abs() < 1e-1);
        assert!((points[1].lat - 46.57700).abs() < 1e-5);
    }

    #[test]
    fn test_interpolation() {
        let points = vec![
            GpxPoint {
                lat: 46.0,
                lon: 6.0,
                ele: 400.0,
                time: 1000,
            },
            GpxPoint {
                lat: 47.0,
                lon: 7.0,
                ele: 500.0,
                time: 2000,
            },
        ];

        // Exact match
        let gps = find_gps_for_time(&points, 1000).unwrap();
        assert!((gps.lat - 46.0).abs() < 1e-10);

        // Midpoint
        let gps = find_gps_for_time(&points, 1500).unwrap();
        assert!((gps.lat - 46.5).abs() < 1e-10);
        assert!((gps.lon - 6.5).abs() < 1e-10);
        assert!((gps.ele - 450.0).abs() < 1e-10);

        // Quarter point
        let gps = find_gps_for_time(&points, 1250).unwrap();
        assert!((gps.lat - 46.25).abs() < 1e-10);
    }

    #[test]
    fn test_parse_exif_datetime() {
        let ts = parse_exif_datetime("2024:01:15 10:30:00").unwrap();
        let expected = parse_iso8601("2024-01-15T10:30:00Z").unwrap();
        assert_eq!(ts, expected);
    }

    #[test]
    fn test_parse_iso8601_with_timezone() {
        let utc = parse_iso8601("2024-01-15T10:30:00Z").unwrap();
        let plus1 = parse_iso8601("2024-01-15T11:30:00+01:00").unwrap();
        assert_eq!(utc, plus1);
    }

    #[test]
    fn test_datetime_to_unix() {
        // 1970-01-01T00:00:00Z should be 0
        assert_eq!(datetime_to_unix(1970, 1, 1, 0, 0, 0), 0);
        // 2024-01-01T00:00:00Z
        assert_eq!(datetime_to_unix(2024, 1, 1, 0, 0, 0), 1704067200);
    }
}
