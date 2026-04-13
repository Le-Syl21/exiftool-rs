//! Geolocation reverse geocoding using ExifTool's Geolocation.dat.
//!
//! Loads the binary database and finds the nearest city to a GPS coordinate.

use std::path::Path;

/// A city entry from the geolocation database.
#[derive(Debug, Clone)]
pub struct City {
    pub name: String,
    pub country_code: String,
    pub country: String,
    pub region: String,
    pub subregion: String,
    pub timezone: String,
    pub population: u64,
    pub lat: f64,
    pub lon: f64,
}

/// The geolocation database.
pub struct GeolocationDb {
    cities: Vec<CityRecord>,
    countries: Vec<(String, String)>, // (code, name)
    regions: Vec<String>,
    subregions: Vec<String>,
    timezones: Vec<String>,
}

/// Raw binary city record (13 bytes + name).
struct CityRecord {
    lat_raw: u32, // 20-bit
    lon_raw: u32, // 20-bit
    country_idx: u8,
    pop_code: u32,
    region_idx: u16,
    subregion_idx: u16,
    tz_idx: u16,
    name: String,
}

impl GeolocationDb {
    /// Load the database from ExifTool's Geolocation.dat file.
    pub fn load<P: AsRef<Path>>(path: P) -> Option<Self> {
        let data = std::fs::read(path.as_ref()).ok()?;
        Self::parse(&data)
    }

    /// Try to find the database in common locations.
    pub fn load_default() -> Option<Self> {
        // Try relative to executable, then common ExifTool locations
        let candidates = [
            "Geolocation.dat",
            "../exiftool/lib/Image/ExifTool/Geolocation.dat",
            "/usr/share/exiftool/Geolocation.dat",
            "/usr/local/share/exiftool/Geolocation.dat",
        ];

        for path in &candidates {
            if let Some(db) = Self::load(path) {
                return Some(db);
            }
        }

        // Try relative to the executable path
        if let Ok(exe) = std::env::current_exe() {
            if let Some(dir) = exe.parent() {
                let p = dir.join("Geolocation.dat");
                if let Some(db) = Self::load(&p) {
                    return Some(db);
                }
            }
        }

        None
    }

    /// Parse the binary database.
    fn parse(data: &[u8]) -> Option<Self> {
        // Find first newline (end of header line)
        let header_end = data.iter().position(|&b| b == b'\n')?;
        let header = std::str::from_utf8(&data[..header_end]).ok()?;

        // Validate header: "GeolocationX.XX\tNNNN"
        if !header.starts_with("Geolocation") {
            return None;
        }
        let tab_pos = header.find('\t')?;
        let city_count: usize = header[tab_pos + 1..].parse().ok()?;

        // Skip comment line
        let mut pos = header_end + 1;
        if pos < data.len() && data[pos] == b'#' {
            while pos < data.len() && data[pos] != b'\n' {
                pos += 1;
            }
            pos += 1;
        }

        // Read city records
        let mut cities = Vec::with_capacity(city_count);
        loop {
            if pos + 6 > data.len() {
                break;
            }
            // Check for section separator: \0\0\0\0\xNN\n
            if data[pos] == 0 && data[pos + 1] == 0 && data[pos + 2] == 0 && data[pos + 3] == 0 {
                pos += 6; // Skip separator
                break;
            }

            // Need at least 14 bytes (13 binary + 1 for name + newline)
            if pos + 14 > data.len() {
                break;
            }

            // Parse 13-byte binary header
            let lt = u16::from_be_bytes([data[pos], data[pos + 1]]);
            let f = data[pos + 2];
            let ln = u16::from_be_bytes([data[pos + 3], data[pos + 4]]);
            let code =
                u32::from_be_bytes([data[pos + 5], data[pos + 6], data[pos + 7], data[pos + 8]]);
            let sn = u16::from_be_bytes([data[pos + 9], data[pos + 10]]);
            let tn = data[pos + 11];
            let fn_byte = data[pos + 12];

            let lat_raw = ((lt as u32) << 4) | ((f >> 4) as u32);
            let lon_raw = ((ln as u32) << 4) | ((f & 0x0F) as u32);
            let country_idx = (code >> 24) as u8;
            let region_idx = (code & 0x0FFF) as u16;
            let subregion_idx = sn & 0x7FFF;

            // Timezone: 9-bit index
            let tz_high = (fn_byte >> 7) as u16; // v1.03: bit 7 of feature byte
            let tz_idx = (tz_high << 8) | (tn as u16);

            // Find city name (UTF-8 until newline)
            let name_start = pos + 13;
            let name_end = data[name_start..]
                .iter()
                .position(|&b| b == b'\n')
                .map(|p| name_start + p)
                .unwrap_or(data.len());

            let name =
                crate::encoding::decode_utf8_or_latin1(&data[name_start..name_end]).to_string();

            cities.push(CityRecord {
                lat_raw,
                lon_raw,
                country_idx,
                pop_code: code,
                region_idx,
                subregion_idx,
                tz_idx,
                name,
            });

            pos = name_end + 1;
        }

        // Read string lists
        // Countries: "CCCountryName\n" (2-char code + name on same line)
        let countries = read_country_list(data, &mut pos);
        skip_separator(data, &mut pos);
        let regions = read_string_list(data, &mut pos);
        skip_separator(data, &mut pos);
        let subregions = read_string_list(data, &mut pos);
        skip_separator(data, &mut pos);
        let timezones = read_string_list(data, &mut pos);

        Some(GeolocationDb {
            cities,
            countries,
            regions,
            subregions,
            timezones,
        })
    }

    /// Find the nearest city to the given coordinates.
    pub fn find_nearest(&self, lat: f64, lon: f64) -> Option<City> {
        if self.cities.is_empty() {
            return None;
        }

        let mut best_idx = 0;
        let mut best_dist = f64::MAX;

        for (i, city) in self.cities.iter().enumerate() {
            let clat = city.lat_raw as f64 * 180.0 / 1048576.0 - 90.0;
            let clon = city.lon_raw as f64 * 360.0 / 1048576.0 - 180.0;

            let dlat = (lat - clat) * std::f64::consts::PI / 180.0;
            let dlon = (lon - clon) * std::f64::consts::PI / 180.0;

            // Simplified distance (no need for Haversine for nearest-city search)
            let cos_lat = ((lat + clat) / 2.0 * std::f64::consts::PI / 180.0).cos();
            let dist = dlat * dlat + (dlon * cos_lat) * (dlon * cos_lat);

            if dist < best_dist {
                best_dist = dist;
                best_idx = i;
            }
        }

        Some(self.get_city(best_idx))
    }

    /// Get a city entry by index.
    fn get_city(&self, idx: usize) -> City {
        let rec = &self.cities[idx];

        let lat = rec.lat_raw as f64 * 180.0 / 1048576.0 - 90.0;
        let lon = rec.lon_raw as f64 * 360.0 / 1048576.0 - 180.0;

        let (country_code, country) = self
            .countries
            .get(rec.country_idx as usize)
            .cloned()
            .unwrap_or_else(|| ("??".into(), "Unknown".into()));

        let region = self
            .regions
            .get(rec.region_idx as usize)
            .cloned()
            .unwrap_or_default();

        let subregion = self
            .subregions
            .get(rec.subregion_idx as usize)
            .cloned()
            .unwrap_or_default();

        let timezone = self
            .timezones
            .get(rec.tz_idx as usize)
            .cloned()
            .unwrap_or_default();

        // Decode population: N.Fe+0E
        let e = (rec.pop_code >> 20) & 0x0F;
        let n = (rec.pop_code >> 16) & 0x0F;
        let f = (rec.pop_code >> 12) & 0x0F;
        let pop_str = format!("{}.{}e+0{}", n, f, e);
        let population: u64 = pop_str.parse::<f64>().unwrap_or(0.0) as u64;

        City {
            name: rec.name.clone(),
            country_code,
            country,
            region,
            subregion,
            timezone,
            population,
            lat: (lat * 10000.0).round() / 10000.0,
            lon: (lon * 10000.0).round() / 10000.0,
        }
    }

    /// Number of cities in the database.
    pub fn len(&self) -> usize {
        self.cities.len()
    }

    /// Returns true if the database contains no cities.
    pub fn is_empty(&self) -> bool {
        self.cities.is_empty()
    }
}

fn read_string_list(data: &[u8], pos: &mut usize) -> Vec<String> {
    let mut list = Vec::new();
    loop {
        if *pos + 6 > data.len() {
            break;
        }
        // Check for separator
        if data[*pos] == 0
            && *pos + 3 < data.len()
            && data[*pos + 1] == 0
            && data[*pos + 2] == 0
            && data[*pos + 3] == 0
        {
            break;
        }
        // Read until newline
        let start = *pos;
        while *pos < data.len() && data[*pos] != b'\n' {
            *pos += 1;
        }
        let s = crate::encoding::decode_utf8_or_latin1(&data[start..*pos]).to_string();
        list.push(s);
        if *pos < data.len() {
            *pos += 1; // Skip newline
        }
    }
    list
}

fn read_country_list(data: &[u8], pos: &mut usize) -> Vec<(String, String)> {
    let mut list = Vec::new();
    loop {
        if *pos + 6 > data.len() {
            break;
        }
        if data[*pos] == 0 && data[*pos + 1] == 0 && data[*pos + 2] == 0 && data[*pos + 3] == 0 {
            break;
        }
        // Read line: "CCCountryName\n"
        let start = *pos;
        while *pos < data.len() && data[*pos] != b'\n' {
            *pos += 1;
        }
        let line = crate::encoding::decode_utf8_or_latin1(&data[start..*pos]).to_string();
        if *pos < data.len() {
            *pos += 1;
        }

        if line.len() >= 2 {
            let code = line[..2].to_string();
            let name = line[2..].to_string();
            list.push((code, name));
        }
    }
    list
}

fn skip_separator(data: &[u8], pos: &mut usize) {
    if *pos + 6 <= data.len() && data[*pos] == 0 {
        *pos += 6;
    }
}
