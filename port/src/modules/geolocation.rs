//! Image::ExifTool::Geolocation (Image/ExifTool/Geolocation.pm), ported sub by sub (see docs/port/PORTING.md).
//!
//! Determine geolocation from GPS and visa-versa. The database lives in the
//! module's file lexicals (`@cityList`, `%countryNum`...), which the module's
//! top-level code fills; the ports read and change them in place.

use std::collections::HashMap;

use crate::interp::eval::{Cell, ER};
use crate::interp::{Interp, Want};
use crate::modules::shift::set_items;
use crate::perl::{ops, Value};
use crate::pl::{self, arg, cap, re, ret};
use crate::runtime;

const MODULE: &str = "Image::ExifTool::Geolocation";

fn n(i: i64) -> Value {
    Value::Int(i)
}

// ---- local helpers ----

fn la(it: &mut Interp, name: &str) -> Value {
    runtime::lex_array(it, MODULE, name)
}

fn lh(it: &mut Interp, name: &str) -> Value {
    runtime::lex_hash(it, MODULE, name)
}

fn ls(it: &mut Interp, name: &str) -> Cell {
    runtime::lex_scalar(it, MODULE, name)
}

fn get(c: &Cell) -> Value {
    c.borrow().clone()
}

fn put(c: &Cell, v: Value) {
    *c.borrow_mut() = v;
}

/// `$#array`
fn last_index(a: &Value) -> i64 {
    a.len() as i64 - 1
}

/// `%$h = ()` (in place)
fn clear_hash(h: &Value) {
    for k in h.keys() {
        h.delete(&k);
    }
}

/// `$$h{$k}` with a value key.
fn hget(h: &Value, k: &Value) -> Value {
    h.get(&*k.to_bytes())
}

fn hset(h: &Value, k: &Value, v: Value) {
    h.set(&*k.to_bytes(), v);
}

/// `chomp $x` with the current `$/`.
fn chomp(it: &mut Interp, v: &Value) -> Value {
    let rs = it.get_scalar("main::/");
    if v.is_undef() || rs.is_undef() {
        return v.clone();
    }
    let b = v.to_bytes();
    let sep = rs.to_bytes();
    if sep.is_empty() {
        let mut e = b.len();
        while e > 0 && b[e - 1] == b'\n' {
            e -= 1;
        }
        return Value::bytes(&b[..e]);
    }
    if b.ends_with(&sep) {
        Value::bytes(&b[..b.len() - sep.len()])
    } else {
        v.clone()
    }
}

/// A file read with `<FH>`: the records split on `$/` (undef: the rest).
struct Lines {
    data: Vec<u8>,
    pos: usize,
}

impl Lines {
    fn open(path: &Value) -> Option<Lines> {
        // `open FH, "< $file"`: perl strips the leading/trailing spaces of the name
        let p = path.to_text();
        std::fs::read(p.trim()).ok().map(|data| Lines { data, pos: 0 })
    }

    fn read(&mut self, sep: &Value) -> Value {
        if self.pos >= self.data.len() {
            return Value::Undef;
        }
        let rest = &self.data[self.pos..];
        let end = if sep.is_undef() {
            rest.len()
        } else {
            let s = sep.to_bytes();
            if s.is_empty() {
                rest.len()
            } else {
                rest.windows(s.len()).position(|w| w == &s[..]).map(|p| p + s.len()).unwrap_or(rest.len())
            }
        };
        let v = Value::bytes(&rest[..end]);
        self.pos += end;
        v
    }
}

/// `$x =~ s/PAT//` capturing `$1`: the capture when it matched.
fn s_cap1(pat: &'static str, x: &mut Value) -> Option<Value> {
    let mut c1 = None;
    if re(pat, "").subst(x, false, |c| {
        c1 = Some(cap(c, 1));
        Vec::new()
    }) > 0
    {
        c1
    } else {
        None
    }
}

/// `$s1 cmp $s2`
fn scmp(a: &[u8], b: &[u8]) -> std::cmp::Ordering {
    a.cmp(b)
}

// ---- the subs ----

/// Read Geolocation database
/// Inputs: 0) database file name
/// Returns: true on success
pub fn read_database(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let datfile = arg(&a, 0);
    // open geolocation database and verify header
    let Some(mut f) = Lines::open(&datfile) else {
        pl::warn(it, &format!("Error reading {}\n", datfile.to_text()))?;
        return ret(n(0));
    };
    let rs = it.get_scalar("main::/");
    let mut line = f.read(&rs);
    let Some(c) = (if line.truthy() { re(r"^Geolocation(\d+\.\d+)\t(\d+)", "").m(&line) } else { None }) else {
        pl::warn(it, "Bad format Geolocation database\n")?;
        return ret(n(0));
    };
    let db_ver = ls(it, "dbVer");
    let n_city = ls(it, "nCity");
    put(&db_ver, cap(&c, 1));
    put(&n_city, cap(&c, 2));
    if !re(r"^1\.0[23]$", "").is_match(&get(&db_ver)) {
        let which = if pl::lt(&get(&db_ver), &Value::Num(1.03)) { "database" } else { "ExifTool" };
        pl::warn(it, &format!("Incompatible Geolocation database (update your {which})\n"))?;
        return ret(n(0));
    }
    let comment = f.read(&rs);
    let Some(c) = (if comment.is_undef() { None } else { re(r" (\d+) ", "").m(&comment) }) else {
        return ret(n(0));
    };
    it.set_scalar(&format!("{MODULE}::dbInfo"), pl::s(&format!("{} v{}: {} cities with population > {}", datfile.to_text(), get(&db_ver).to_text(), get(&n_city).to_text(), cap(&c, 1).to_text())));
    let is_user_defined = !pl::pkg_array(it, "Image::ExifTool::UserDefined::Geolocation").is_empty();

    let alt_names = la(it, "altNames");
    set_items(&alt_names, vec![]); // reset altNames

    let is_end = |l: &Value| !l.is_undef() && l.to_bytes().len() == 6 && re(r"\0\0\0\0", "").is_match(l);

    // read city database
    let mut err = false;
    let city_list = la(it, "cityList");
    let mut cities = Vec::new();
    loop {
        line = f.read(&rs);
        if line.is_undef() {
            err = true;
            break;
        }
        if is_end(&line) {
            break;
        }
        while pl::lt(&pl::length(&line), &n(14)) {
            let more = f.read(&rs);
            if more.is_undef() {
                err = true;
                break;
            }
            line = pl::concat(&line, &more);
        }
        cities.push(chomp(it, &line));
    }
    set_items(&city_list, cities);
    if !pl::num_eq(&n(city_list.len() as i64), &get(&n_city)) {
        pl::warn(it, "Bad number of entries in Geolocation database\n")?;
        return ret(n(0));
    }
    let lists = [("countryList", "countryNum"), ("regionList", "regionNum"), ("subregionList", "subregionNum"), ("timezoneList", "timezoneNum")];
    for (k, (list_name, num_name)) in lists.iter().enumerate() {
        let list = la(it, list_name);
        let num = lh(it, num_name);
        loop {
            line = f.read(&rs);
            if line.is_undef() {
                err = true;
                break;
            }
            if is_end(&line) {
                break;
            }
            line = chomp(it, &line);
            list.push(line.clone());
            if is_user_defined {
                let key = if k == 0 { pl::lc(&pl::substr(&line, 0, Some(2))) } else { pl::lc(&line) };
                hset(&num, &key, n(last_index(&list)));
            }
        }
    }
    // read feature codes if available
    let feature_codes = la(it, "featureCodes");
    if !line.is_undef() && pl::str_eq(&line, "\0\0\0\0\x05\n") {
        set_items(&feature_codes, vec![]);
        let feature_types = lh(it, "featureTypes");
        loop {
            line = f.read(&rs);
            if line.is_undef() {
                err = true;
                break;
            }
            if is_end(&line) {
                break;
            }
            line = chomp(it, &line);
            if let Some(t) = s_cap1(r" (.*)", &mut line) {
                hset(&feature_types, &line, t);
            }
            feature_codes.push(line.clone());
        }
    }
    if err {
        pl::at(220);
        pl::warn(it, &format!("Error reading {} (truncated file?)", datfile.to_text()))?;
        ret(n(0))
    } else {
        // initialize featureCodes lookup
        let fc_lookup = lh(it, "featureCodes");
        clear_hash(&fc_lookup);
        for (i, c) in feature_codes.items().iter().enumerate() {
            hset(&fc_lookup, &pl::lc(c), n(i as i64));
        }
        ret(n(1))
    }
}

/// Read alternate-names database
/// Returns: True on success
/// Notes: Must be called after ReadDatabase(). Resets $altDir on exit.
pub fn read_alt_names(it: &mut Interp, _a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let mut success = Value::Undef;
    let alt_dir = it.get_scalar(&format!("{MODULE}::altDir"));
    if alt_dir.truthy() && get(&ls(it, "nCity")).truthy() {
        let path = pl::concat(&alt_dir, &pl::s("/AltNames.dat"));
        if let Some(mut f) = Lines::open(&path) {
            let sep = pl::s("\0");
            let alt_names = la(it, "altNames");
            let mut i = 0usize;
            loop {
                let rec = f.read(&sep);
                if rec.is_undef() {
                    break;
                }
                let b = rec.to_bytes();
                alt_names.set_at(i, Value::bytes(&b[..b.len() - 1])); // chop
                i += 1;
            }
            if pl::num_eq(&n(i as i64), &get(&ls(it, "nCity"))) {
                success = n(1);
            } else {
                pl::warn(it, "Bad number of entries in AltNames database\n")?;
                set_items(&alt_names, vec![]);
            }
        } else {
            pl::warn(it, &format!("Error reading {}/AltNames.dat\n", alt_dir.to_text()))?;
        }
        it.set_scalar(&format!("{MODULE}::altDir"), Value::Undef);
    }
    ret(success)
}

/// Clear last city matches cache
pub fn clear_last_matches(it: &mut Interp, _a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    clear_last(it);
    pl::ret_undef()
}

fn clear_last(it: &mut Interp) {
    put(&ls(it, "lastArgs"), Value::Undef); // arguments in last call to Geolocate
    clear_hash(&lh(it, "lastFound")); // keys are last matching city numbers, values are population codes
    set_items(&la(it, "lastByPop"), vec![]); // last matching city numbers ordered by population
    set_items(&la(it, "lastByLat"), vec![]); // last matching city numbers ordered by latitude
}

/// Sort database by specified field
/// Inputs: 0) Field name to sort (Latitude,City,Country)
/// Returns: 1 on success
pub fn sort_database(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    ret(n(sort_db(it, &arg(&a, 0)) as i64))
}

fn sort_db(it: &mut Interp, field: &Value) -> bool {
    let sorted_by = ls(it, "sortedBy");
    if field.to_bytes() == get(&sorted_by).to_bytes() {
        return true; // already sorted?
    }
    let sort_order = la(it, "sortOrder");
    set_items(&sort_order, vec![]);
    let cities: Vec<std::rc::Rc<[u8]>> = la(it, "cityList").items().iter().map(|v| v.to_bytes()).collect();
    let mut idx: Vec<usize> = (0..cities.len()).collect();
    let tail = |b: &[u8]| -> Vec<u8> { b.get(13..).map(|x| x.to_vec()).unwrap_or_default() };
    if pl::str_eq(field, "Latitude") {
        idx.sort_by(|&a, &b| scmp(&cities[a], &cities[b]));
    } else if pl::str_eq(field, "City") {
        let keys: Vec<Vec<u8>> = cities.iter().map(|c| tail(c)).collect();
        idx.sort_by(|&a, &b| scmp(&keys[a], &keys[b]));
    } else if pl::str_eq(field, "Country") {
        let country_list = la(it, "countryList");
        let lkup: Vec<Vec<u8>> = cities
            .iter()
            .map(|c| {
                let city = tail(c);
                let ord = c.get(5).copied().unwrap_or(0) as i64;
                let ctry = pl::substr(&country_list.at(ord), 2, None);
                let mut k = ctry.to_bytes().to_vec();
                k.push(b' ');
                k.extend_from_slice(&city);
                k
            })
            .collect();
        idx.sort_by(|&a, &b| scmp(&lkup[a], &lkup[b]));
    } else {
        return false;
    }
    set_items(&sort_order, idx.into_iter().map(|i| n(i as i64)).collect());
    put(&sorted_by, field.clone());
    clear_last(it);
    true
}

/// Add cities to the Geolocation database
/// Inputs: 0-8) city,region,subregion,country_code,country,timezone,feature_code,population,lat,lon,altNames
/// eg. AddEntry('Sinemorets','Burgas','Obshtina Tsarevo','BG','Bulgaria','Europe/Sofia','',400,42.06115,27.97833)
/// Returns: true on success, otherwise issues warning
pub fn add_entry(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (city, region, subregion, cc, country, timezone, mut fc, mut pop, mut lat, mut lon, mut alt_names) = (arg(&a, 0), arg(&a, 1), arg(&a, 2), arg(&a, 3), arg(&a, 4), arg(&a, 5), arg(&a, 6), arg(&a, 7), arg(&a, 8), arg(&a, 9), arg(&a, 10));
    if a.len() < 10 {
        pl::warn(it, &format!("Too few arguments in {} definition (check for updated format)\n", city.to_text()))?;
        return ret(n(0));
    }
    if cc.to_bytes().len() != 2 {
        pl::warn(it, &format!("Country code '{}' is not 2 characters\n", cc.to_text()))?;
        return ret(n(0));
    }
    let feature_types = lh(it, "featureTypes");
    if let Some(t) = s_cap1(r" (.*)", &mut fc) {
        hset(&feature_types, &fc, t);
    }
    let fc_lookup = lh(it, "featureCodes");
    let feature_codes = la(it, "featureCodes");
    let db_ver = get(&ls(it, "dbVer"));
    let mut fnum = hget(&fc_lookup, &pl::lc(&fc));
    if fnum.is_undef() {
        if pl::str_eq(&db_ver, "1.02") || feature_codes.len() > 0x3f || fc.to_bytes().is_empty() {
            fnum = n(0);
        } else {
            feature_codes.push(pl::uc(&fc));
            fnum = n(last_index(&feature_codes));
            hset(&fc_lookup, &pl::lc(&fc), fnum.clone());
        }
    }
    lon = chomp(it, &lon); // (just in case it was read from file)
    let country_list = la(it, "countryList");
    let region_list = la(it, "regionList");
    let subregion_list = la(it, "subregionList");
    let timezone_list = la(it, "timezoneList");
    let country_num = lh(it, "countryNum");
    let region_num = lh(it, "regionNum");
    let subregion_num = lh(it, "subregionNum");
    let timezone_num = lh(it, "timezoneNum");
    // create reverse lookups for country/region/subregion/timezone if not done already
    // (eg. if the entries are being added manually instead of via UserDefined::Geolocation)
    if country_num.is_empty() {
        for (i, c) in country_list.items().iter().enumerate() {
            hset(&country_num, &pl::lc(&pl::substr(c, 0, Some(2))), n(i as i64));
        }
        for (i, c) in region_list.items().iter().enumerate() {
            hset(&region_num, &pl::lc(c), n(i as i64));
        }
        for (i, c) in subregion_list.items().iter().enumerate() {
            hset(&subregion_num, &pl::lc(c), n(i as i64));
        }
        for (i, c) in timezone_list.items().iter().enumerate() {
            hset(&timezone_num, &pl::lc(c), n(i as i64));
        }
    }
    let mut cn = hget(&country_num, &pl::lc(&cc));
    if cn.is_undef() {
        if last_index(&country_list) >= 0xff {
            pl::warn(it, "AddEntry: Too many countries\n")?;
            return ret(n(0));
        }
        country_list.push(pl::concat(&cc, &country));
        cn = n(last_index(&country_list));
        hset(&country_num, &pl::lc(&cc), cn.clone());
    } else if country.truthy() {
        country_list.set_at(cn.to_i64() as usize, pl::concat(&cc, &country)); // (override existing country name)
    }
    let mut tn = hget(&timezone_num, &pl::lc(&timezone));
    if tn.is_undef() {
        if last_index(&timezone_list) >= 0x1ff {
            pl::warn(it, "AddEntry: Too many time zones\n")?;
            return ret(n(0));
        }
        timezone_list.push(timezone.clone());
        tn = n(last_index(&timezone_list));
        hset(&timezone_num, &pl::lc(&timezone), tn.clone());
    }
    let mut rn = hget(&region_num, &pl::lc(&region));
    if rn.is_undef() {
        if last_index(&region_list) >= 0xfff {
            pl::warn(it, "AddEntry: Too many regions\n")?;
            return ret(n(0));
        }
        region_list.push(region.clone());
        rn = n(last_index(&region_list));
        hset(&region_num, &pl::lc(&region), rn.clone());
    }
    let mut sn = hget(&subregion_num, &pl::lc(&subregion));
    if sn.is_undef() {
        let max = if pl::str_eq(&db_ver, "1.02") { 0x7fff } else { 0xffff };
        if last_index(&subregion_list) >= max {
            pl::warn(it, "AddEntry: Too many subregions\n")?;
            return ret(n(0));
        }
        subregion_list.push(subregion.clone());
        sn = n(last_index(&subregion_list));
        hset(&subregion_num, &pl::lc(&subregion), sn.clone());
    }
    pop = pl::sprintf("%.1e", &[pop]); // format: "3.1e+04" or "3.1e+004"
    // pack CC index, population and region index into a 32-bit integer
    let sh = |v: Value, s: i64| ops::shift_left(&v, &n(s));
    let mut code = sh(cn.clone(), 24);
    code = ops::bit_or(&code, &sh(pl::substr(&pop, -1, Some(1)), 20));
    code = ops::bit_or(&code, &sh(pl::substr(&pop, 0, Some(1)), 16));
    code = ops::bit_or(&code, &sh(pl::substr(&pop, 2, Some(1)), 12));
    code = ops::bit_or(&code, &rn);
    // store high bit of timezone index
    if pl::gt(&tn, &n(255)) {
        if pl::str_eq(&db_ver, "1.02") {
            sn = ops::bit_or(&sn, &n(0x8000));
        } else {
            fnum = ops::bit_or(&fnum, &n(0x80));
        }
        tn = pl::sub(&tn, &n(256));
    }
    let scale = |v: &Value, off: i64, range: i64| -> ER<Value> { Ok(ops::bit_and(&pl::int(&pl::add(&pl::mul(&pl::div(&pl::add(v, &n(off)), &n(range))?, &n(0x100000)), &Value::Num(0.5))), &n(0xfffff))) };
    lat = scale(&lat, 90, 180)?;
    lon = scale(&lon, 180, 360)?;
    let shr = |v: &Value, s: i64| ops::shift_right(v, &n(s));
    let hdr = pl::pack("nCnNnCC", &[shr(&lat, 4), ops::bit_or(&ops::shift_left(&ops::bit_and(&lat, &n(0x0f)), &n(4)), &ops::bit_and(&lon, &n(0x0f))), shr(&lon, 4), code, sn, tn, fnum])?;
    let city_list = la(it, "cityList");
    city_list.push(pl::concat(&hdr, &city));
    // add altNames entry if provided
    if alt_names.truthy() {
        alt_names = chomp(it, &alt_names); // (just in case)
        pl::tr(&mut alt_names, ",", "\n", "");
        // add any more arguments in case altNames were passed separately (undocumented)
        for i in 11..a.len() {
            let c = chomp(it, &a[i]);
            it.set_arg(i, c.clone())?;
            alt_names = pl::concat(&alt_names, &pl::concat(&pl::s("\n"), &c));
        }
        la(it, "altNames").set_at(last_index(&city_list) as usize, alt_names);
    }
    put(&ls(it, "sortedBy"), pl::s(""));
    put(&ls(it, "lastArgs"), Value::Undef); // (faster than ClearLastArgs)
    ret(n(1))
}

/// Unpack entry in database
/// Inputs: 0) entry number or index into sorted database,
///         1) optional language code, 2) flag to use index into sorted database
/// Returns: 0-10) city,region,subregion,country_code,country,timezone,
///                feature_code,pop,lat,lon,feature_type
pub fn get_entry(it: &mut Interp, a: Vec<Value>, want: Want) -> ER<Vec<Value>> {
    let r = get_entry_v(it, &arg(&a, 0), &arg(&a, 1), &arg(&a, 2))?;
    if want == Want::List {
        Ok(r)
    } else {
        Ok(vec![r.last().cloned().unwrap_or_default()])
    }
}

pub(crate) fn get_entry_v(it: &mut Interp, entry_num: &Value, lang: &Value, sort: &Value) -> ER<Vec<Value>> {
    let mut entry_num = entry_num.clone();
    let city_list = la(it, "cityList");
    if pl::gt(&entry_num, &n(last_index(&city_list))) {
        return Ok(vec![]);
    }
    let sort_order = la(it, "sortOrder");
    if sort.truthy() && pl::gt(&n(sort_order.len() as i64), &entry_num) {
        entry_num = sort_order.at(entry_num.to_i64());
    }
    let entry = city_list.at(entry_num.to_i64());
    let u = pl::unpack("nCnNnCC", &entry)?;
    let (mut lt, f, mut ln, code, mut sn, mut tn, fnum) = (arg(&u, 0), arg(&u, 1), arg(&u, 2), arg(&u, 3), arg(&u, 4), arg(&u, 5), arg(&u, 6));
    let mut city = pl::substr(&entry, 13, None);
    let ctry = la(it, "countryList").at(ops::shift_right(&code, &n(24)).to_i64());
    let mut rgn = la(it, "regionList").at(ops::bit_and(&code, &n(0x0fff)).to_i64());
    if pl::str_eq(&get(&ls(it, "dbVer")), "1.02") {
        if ops::bit_and(&sn, &n(0x8000)).truthy() {
            tn = pl::add(&tn, &n(256));
            sn = ops::bit_and(&sn, &n(0x7fff));
        }
    } else if ops::bit_and(&fnum, &n(0x80)).truthy() {
        tn = pl::add(&tn, &n(256));
    }
    let mut sub = la(it, "subregionList").at(sn.to_i64());
    // convert population digits back into exponent format
    let dig = |s: i64| ops::bit_and(&ops::shift_right(&code, &n(s)), &n(0x0f));
    let pop = pl::add(&pl::join("", &[dig(16), pl::s("."), dig(12), pl::s("e+"), dig(20)]), &n(0));
    let fb = |s: i64| -> Value { ops::shift_right(&f, &n(s)) };
    lt = pl::sprintf("%.4f", &[pl::sub(&pl::div(&pl::mul(&ops::bit_or(&ops::shift_left(&lt, &n(4)), &fb(4)), &n(180)), &n(0x100000))?, &n(90))]);
    ln = pl::sprintf("%.4f", &[pl::sub(&pl::div(&pl::mul(&ops::bit_or(&ops::shift_left(&ln, &n(4)), &ops::bit_and(&f, &n(0x0f))), &n(360)), &n(0x100000))?, &n(180))]);
    let fcv = la(it, "featureCodes").at(ops::bit_and(&fnum, &n(0x3f)).to_i64());
    let fc = if fcv.truthy() { fcv } else { pl::s("Other") };
    let cc = pl::substr(&ctry, 0, Some(2));
    let mut country = pl::substr(&ctry, 2, None);
    let mut ft = hget(&lh(it, "featureTypes"), &fc);
    if lang.truthy() && !pl::str_eq(lang, "en") {
        let lang_lookup = lh(it, "langLookup");
        let mut xlat = hget(&lang_lookup, lang);
        // load language lookups if  not done already
        if xlat.is_undef() {
            let inc = pl::pkg_array(it, "main::INC");
            inc.unshift(it.get_scalar(&format!("{MODULE}::geoDir"))); // make sure $geoDir is first in path
            let code = pl::concat(&pl::concat(&pl::s("require 'GeoLang/"), lang), &pl::s(".pm'"));
            let (r, _) = pl::eval_site(it, &code, MODULE, &[], Want::Scalar);
            if r.truthy() {
                let trans = format!("Image::ExifTool::GeoLang::{}::Translate", lang.to_text());
                let h = pl::pkg_hash(it, &trans);
                if !h.is_empty() {
                    xlat = h;
                }
            }
            if let Value::ArrayRef(a) = &inc {
                let mut a = a.borrow_mut();
                if !a.is_empty() {
                    a.remove(0);
                }
            }
            // read user-defined language translations
            let geo_lang = pl::pkg_hash(it, &format!("{MODULE}::geoLang"));
            if !geo_lang.is_empty() {
                let user_lang = hget(&geo_lang, lang);
                if user_lang.truthy() && pl::ref_of(&user_lang) == "HASH" {
                    if xlat.truthy() {
                        // add user-defined entries to main lookup
                        for k in user_lang.keys() {
                            xlat.set(&k, user_lang.get(&k));
                        }
                    } else {
                        xlat = user_lang;
                    }
                }
            }
            hset(&lang_lookup, lang, if xlat.truthy() { xlat.clone() } else { n(0) });
        }
        if xlat.truthy() {
            let r2 = rgn.clone();
            let k = |parts: &[&Value]| -> Value { pl::join("", &parts.iter().map(|v| (*v).clone()).collect::<Vec<_>>()) };
            let (com, empty) = (pl::s(","), pl::s(""));
            let first = |cands: Vec<Value>, dflt: &Value| -> Value {
                for c in cands {
                    let v = hget(&xlat, &c);
                    if v.truthy() {
                        return v;
                    }
                }
                dflt.clone()
            };
            // City-specific: "CCRgn,Sub,City", "CCRgn,City", "CC,City", ",City"
            // Subregion-specific: "CCRgn,Sub,"
            // Region-specific: "CCRgn,"
            // Country-specific: "CC,"
            city = first(vec![k(&[&cc, &r2, &com, &sub, &com, &city]), k(&[&cc, &r2, &com, &city]), k(&[&cc, &com, &city]), k(&[&com, &city]), city.clone()], &city);
            sub = first(vec![k(&[&cc, &rgn, &com, &sub, &com]), sub.clone()], &sub);
            rgn = first(vec![k(&[&cc, &rgn, &com]), rgn.clone()], &rgn);
            country = first(vec![k(&[&cc, &com]), country.clone()], &country);
            let _ = &empty;
            let x = hget(&xlat, &fc);
            if x.truthy() {
                ft = x;
            }
        }
    }
    let tz = la(it, "timezoneList").at(tn.to_i64());
    Ok(vec![city, rgn, sub, cc, country, tz, fc, pop, lt, ln, ft])
}

/// Get alternate names for specified database entry
/// Inputs: 0) entry number or index into sorted database, 1) sort flag
/// Returns: comma-separated list of alternate names, or empty string if no names
/// Notes: ReadAltNames() must be called before this
pub fn get_alt_names(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (mut entry_num, sort) = (arg(&a, 0), arg(&a, 1));
    let sort_order = la(it, "sortOrder");
    if sort.truthy() && pl::gt(&n(sort_order.len() as i64), &entry_num) {
        entry_num = sort_order.at(entry_num.to_i64());
    }
    let mut alt = la(it, "altNames").at(entry_num.to_i64());
    if !alt.truthy() {
        return ret(pl::s(""));
    }
    pl::tr(&mut alt, "\n", ",", "");
    ret(alt)
}

/// Look up lat,lon or city in geolocation database
/// Inputs: 0) "lat,lon", "city,region,country", etc, (city must be first)
///         1) options hash reference (or undef for no options)
/// Options: GeolocMinPop, GeolocMaxDist, GeolocMulti, GeolocFeature, GeolocAltNames,
///          GeolocNearby
/// Returns: 0) Reference to list of indices for matching cities, or undef for no matches
///          1) Reference to list of distance/bearing pairs, or undef if no GPS
///          In scalar context returns list of indices only
pub fn geolocate(it: &mut Interp, a: Vec<Value>, want: Want) -> ER<Vec<Value>> {
    let (mut arg0, opts) = (arg(&a, 0), arg(&a, 1));
    let mut city: Option<Vec<u8>> = None;
    let mut exact: Vec<Value> = Vec::new();
    let mut regex: HashMap<i64, Vec<pl::Re>> = HashMap::new();
    let (mut other, mut cargs) = (false, Vec::<Value>::new());
    let (mut min_pop, mut match_parms, mut coords): (Option<Vec<u8>>, Vec<Value>, Vec<Value>) = (None, Vec::new(), Vec::new());
    let mut fc_ok: HashMap<i64, bool> = HashMap::new();
    let mut both = false;
    let (mut pop, mut max_dist, mut _multi, mut fcodes, mut alt_names) = (Value::Undef, Value::Undef, Value::Undef, Value::Undef, Value::Undef);
    let mut start_time: Vec<Value> = Vec::new();

    if opts.truthy() {
        (pop, max_dist, _multi, fcodes, alt_names) = (opts.get("GeolocMinPop"), opts.get("GeolocMaxDist"), opts.get("GeolocMulti"), opts.get("GeolocFeature"), opts.get("GeolocAltNames"));
    }

    if get(&ls(it, "debug")).truthy() {
        runtime::require(it, "Time::HiRes")?;
        start_time = pl::call_list(it, "Time::HiRes::gettimeofday", vec![])?;
    }
    let city_list = la(it, "cityList");
    if city_list.is_empty() {
        pl::at(494);
        pl::warn(it, "No Geolocation database")?;
        return pl::ret_nothing(want);
    }
    // make population code for comparing with 2 bytes at offset 6 in database
    if pop.truthy() {
        pop = pl::sprintf("%.1e", &[pop]);
        let d = |o: i64| pl::substr(&pop, o, Some(1)).to_i64();
        let b1 = ((d(-1) << 4) | d(0)) as u32;
        let b2 = (d(2) << 4) as u32;
        let mut mp = crate::modules::write_exif::chr(&n(b1 as i64)).to_bytes().to_vec();
        mp.extend_from_slice(&crate::modules::write_exif::chr(&n(b2 as i64)).to_bytes());
        min_pop = Some(mp);
    }
    let feature_codes = la(it, "featureCodes");
    if fcodes.truthy() {
        let mut fcx = fcodes.clone();
        let neg = re(r"^-", "").subst(&mut fcx, false, |_| Vec::new()) > 0;
        fcodes = fcx.clone();
        let list = pl::split(&re(r"\s*,-?\s*", ""), &pl::lc(&fcodes), 0); // (allow leading dash on subsequent codes)
        let lookup = lh(it, "featureCodes");
        if neg {
            for i in 0..feature_codes.len() {
                fc_ok.insert(i as i64, true);
            }
            for c in &list {
                let v = hget(&lookup, c);
                if !v.is_undef() {
                    fc_ok.remove(&v.to_i64());
                }
            }
        } else {
            for c in &list {
                let v = hget(&lookup, c);
                if !v.is_undef() {
                    fc_ok.insert(v.to_i64(), true);
                }
            }
        }
    }
    let fc_test = |entry: &[u8], fc_ok: &HashMap<i64, bool>| -> bool {
        let o = entry.get(12).copied().unwrap_or(0) as i64;
        *fc_ok.get(&(o & 0x3f)).unwrap_or(&false)
    };
    //
    // process input argument
    //
    let mut num = n(1);
    re(r"^\s+", "").subst(&mut arg0, false, |_| Vec::new());
    re(r"\s+$", "").subst(&mut arg0, false, |_| Vec::new()); // remove leading/trailing spaces
    let args = pl::split(&re(r"\s*,\s*", ""), &arg0, 0);
    let ri = |k: &[u8]| -> Option<i64> {
        match k {
            b"cc" => Some(0),
            b"co" => Some(1),
            b"re" => Some(2),
            b"sr" => Some(3),
            b"ci" => Some(8),
            b"" => Some(9),
            _ => None,
        }
    };
    for a_ in &args {
        // allow regular expressions optionally prefixed by "ci", "cc", "co", "re" or "sr"
        if let Some(c) = re(r"^(-)?(\w{2})?/(.*)/(i?)$", "").m(a_) {
            pl::at(520);
            let rx = pl::re_dyn(&cap(&c, 3).to_bytes(), if cap(&c, 4).truthy() { "im" } else { "m" })?;
            let Some(mut idx) = ri(&cap(&c, 2).to_bytes()) else { continue };
            cargs.push(a_.clone());
            if idx < 5 {
                other = true;
            }
            if !cap(&c, 1).is_undef() && cap(&c, 1).truthy() {
                idx += 10; // add 10 for negative matches
            }
            regex.entry(idx).or_default().push(rx);
            if city.is_none() {
                city = Some(Vec::new());
            }
        } else if re(r"^[-+]?\d+(\.\d+)?$", "").is_match(a_) {
            // coordinate format
            if coords.len() < 2 {
                coords.push(a_.clone());
            }
        } else if let Some(c) = re(r"^([-+]?\d+(?:\.\d+)?) *(([NS])[A-Z]*)? +([-+]?\d+(?:\.\d+)?) *(([EW])[A-Z]*)?", "i").m(a_) {
            // "lat lon" format
            if !coords.is_empty() {
                continue;
            }
            let (mut lat, mut lon) = (cap(&c, 1), cap(&c, 4));
            if cap(&c, 3).truthy() && pl::str_eq(&pl::uc(&cap(&c, 3)), "S") {
                lat = pl::neg(&pl::abs(&lat));
            }
            if cap(&c, 6).truthy() && pl::str_eq(&pl::uc(&cap(&c, 6)), "W") {
                lon = pl::neg(&pl::abs(&lon));
            }
            coords.push(lat);
            coords.push(lon);
        } else if pl::str_eq(&pl::lc(a_), "both") {
            both = true;
        } else if let Some(c) = re(r"^num=(\d+)$", "i").m(a_) {
            num = cap(&c, 1);
        } else if a_.truthy() {
            cargs.push(a_.clone());
            if city.as_ref().is_some_and(|c| Value::bytes(c).truthy()) {
                exact.push(pl::lc(a_));
            } else {
                city = Some(pl::lc(a_).to_bytes().to_vec());
            }
        }
    }
    if city.is_none() && coords.len() != 2 {
        pl::warn(it, "Insufficient information to determine geolocation\n")?;
        return pl::ret_nothing(want);
    }
    // sort database by logitude if finding entry based on coordinates
    if coords.len() == 2 && (both || city.is_none()) {
        sort_db(it, &pl::s("Latitude"));
    }
    let last_args = ls(it, "lastArgs");
    let last_found = lh(it, "lastFound");
    let last_by_pop = la(it, "lastByPop");
    let last_by_lat = la(it, "lastByLat");
    let country_list = la(it, "countryList");
    let region_list = la(it, "regionList");
    let subregion_list = la(it, "subregionList");
    let db102 = pl::str_eq(&get(&ls(it, "dbVer")), "1.02");
    //
    // perform reverse Geolocation lookup to determine GPS based on city, country, etc.
    //
    while let Some(city_s) = city.clone() {
        if !(coords.len() != 2 || both) {
            break;
        }
        let city_v = Value::bytes(&city_s);
        let mut cj = cargs.clone();
        for x in [&pop, &max_dist, &fcodes] {
            cj.push(if x.truthy() { x.clone() } else { pl::s("") });
        }
        let cargs_s = pl::join(",", &cj);
        let mut i = 0usize;
        let cities = city_list.items();
        if get(&last_args).truthy() && get(&last_args).to_bytes() == cargs_s.to_bytes() {
            i = cities.len(); // bypass search
        } else {
            clear_last(it);
            put(&last_args, cargs_s);
        }
        // read alternate names database if an exact city match is specified
        let alt_list: Vec<Value>;
        if alt_names.truthy() {
            if city_v.truthy() && it.get_scalar(&format!("{MODULE}::altDir")).truthy() {
                read_alt_names(it, vec![], Want::Scalar)?;
            }
            alt_names = la(it, "altNames");
            alt_list = alt_names.items();
        } else {
            alt_names = Value::new_array(vec![]); // (don't search alt names)
            alt_list = alt_names.items();
        }
        let mut alt_re: Option<pl::Re> = None;
        let countries = country_list.items();
        let regions = region_list.items();
        let subregions = subregion_list.items();
        let empty: Vec<pl::Re> = Vec::new();
        let rg = |k: i64| regex.get(&k).unwrap_or(&empty);
        'entry: while i < cities.len() {
            let entry = cities[i].to_bytes();
            let ii = i;
            i += 1;
            let cty = Value::bytes(entry.get(13..).unwrap_or(&[]));
            if city_v.truthy() && city_s != pl::lc(&cty).to_bytes().to_vec() {
                // test exact city name first
                let alt = alt_list.get(ii).cloned().unwrap_or_default();
                if !alt.truthy() {
                    continue;
                }
                if alt_re.is_none() {
                    let mut p = b"^".to_vec();
                    p.extend_from_slice(&city_s);
                    p.push(b'$');
                    pl::at(577);
                    alt_re = Some(pl::re_dyn(&p, "im")?);
                }
                if !alt_re.as_ref().unwrap().is_match(&alt) {
                    continue;
                }
            }
            // test with city-specific regexes
            for r in rg(8) {
                if !r.is_match(&cty) {
                    continue 'entry;
                }
            }
            for r in rg(18) {
                if r.is_match(&cty) {
                    continue 'entry;
                }
            }
            // test other arguments
            let (cd, mut sn) = if entry.len() >= 11 {
                (u32::from_be_bytes([entry[5], entry[6], entry[7], entry[8]]) as i64, u16::from_be_bytes([entry[9], entry[10]]) as i64)
            } else {
                let u = pl::unpack("x5Nn", &cities[ii])?;
                (arg(&u, 0).to_i64(), arg(&u, 1).to_i64())
            };
            let ct = countries.get((cd >> 24) as usize).cloned().unwrap_or_default();
            if db102 {
                sn &= 0x7fff;
            }
            let geo = [pl::substr(&ct, 0, Some(2)), pl::substr(&ct, 2, None), regions.get((cd & 0x0fff) as usize).cloned().unwrap_or_default(), subregions.get(sn as usize).cloned().unwrap_or_default()];
            if !exact.is_empty() {
                // make quick lookup for all names at this location
                let geo_lkup: Vec<Vec<u8>> = geo.iter().filter(|g| g.truthy()).map(|g| pl::lc(g).to_bytes().to_vec()).collect();
                for e in &exact {
                    if !geo_lkup.iter().any(|g| &g[..] == &e.to_bytes()[..]) {
                        continue 'entry;
                    }
                }
            }
            // test with cc, co, re and sr regexes
            if other {
                for idx in 0..4i64 {
                    for r in rg(idx) {
                        if !r.is_match(&geo[idx as usize]) {
                            continue 'entry;
                        }
                    }
                    for r in rg(idx + 10) {
                        if r.is_match(&geo[idx as usize]) {
                            continue 'entry;
                        }
                    }
                }
            }
            // test regexes for any place name
            if regex.contains_key(&9) || regex.contains_key(&19) {
                let mut all = vec![cty.clone()];
                all.extend(geo.iter().cloned());
                let s = pl::join("\n", &all);
                for r in rg(9) {
                    if !r.is_match(&s) {
                        continue 'entry;
                    }
                }
                for r in rg(19) {
                    if r.is_match(&s) {
                        continue 'entry;
                    }
                }
            }
            // test feature code and population
            if fcodes.truthy() && !fc_test(&entry, &fc_ok) {
                continue;
            }
            let pc = entry.get(6..8).unwrap_or(&[]).to_vec();
            if min_pop.as_ref().is_none_or(|mp| pc[..] >= mp[..]) {
                last_found.set(ii.to_string(), Value::bytes(&pc));
                if coords.len() == 2 {
                    last_by_lat.push(n(ii as i64));
                }
            }
        }
        if !start_time.is_empty() {
            let t = pl::call(it, "Time::HiRes::tv_interval", vec![Value::new_array(start_time.clone())])?;
            pl::print_to(it, None, &[pl::sprintf("= Processing time: %.3f sec\n", &[t])])?;
        }
        if !last_found.is_empty() {
            if coords.len() == 2 {
                break; // continue to use coords with last city matches
            }
            if last_found.len() > 200 {
                pl::warn(it, "Too many matching cities\n")?;
                return pl::ret_nothing(want);
            }
            // return nearby cities if "num=" is used and only one match found
            if pl::gt(&num, &n(1)) && last_found.len() == 1 {
                let k = Value::bytes(&last_found.keys()[0]);
                let entry = get_entry_v(it, &k, &Value::Undef, &Value::Undef)?;
                coords = vec![arg(&entry, 8), arg(&entry, 9)];
                sort_db(it, &pl::s("Latitude")); // (make sure we are sorted by latitude)
                break;
            }
            if last_by_pop.is_empty() {
                let mut keys: Vec<Value> = last_found.keys().into_iter().map(Value::from).collect();
                let cmp_city = |k: &Value| cities.get(k.to_i64() as usize).map(|v| v.to_bytes()).unwrap_or_else(|| std::rc::Rc::from(&b""[..]));
                keys.sort_by(|a, b| {
                    let o = scmp(&hget(&last_found, b).to_bytes(), &hget(&last_found, a).to_bytes());
                    if o != std::cmp::Ordering::Equal {
                        return o;
                    }
                    scmp(&cmp_city(a), &cmp_city(b))
                });
                set_items(&last_by_pop, keys);
            }
            return Ok(vec![last_by_pop.clone()]);
        }
        pl::warn(it, "No such city in Geolocation database\n")?;
        return pl::ret_nothing(want);
    }
    //
    // determine Geolocation based on GPS coordinates
    //
    let pi = 3.1415926536f64;
    let earth_radius = 6371f64;
    let (lat0, lon0) = (arg(&coords, 0), arg(&coords, 1));
    let (mut min_dist_u, mut min_dist_c);
    if max_dist.truthy() {
        min_dist_u = max_dist.to_f64() / (2.0 * earth_radius); // min distance on unit sphere
        min_dist_c = max_dist.to_f64() * 0x100000 as f64 / (pi * earth_radius); // min distance in coordinate units
    } else {
        min_dist_u = pi;
        min_dist_c = 0x200000 as f64;
    }
    let _cos = (lat0.to_f64() * pi / 180.0).cos(); // cosine factor for longitude distances
    // reduce lat/lon to the range 0-0x100000
    let scale = |v: &Value, off: i64, range: i64| -> ER<Value> { Ok(ops::bit_and(&pl::int(&pl::add(&pl::mul(&pl::div(&pl::add(v, &n(off)), &n(range))?, &n(0x100000)), &Value::Num(0.5))), &n(0xfffff))) };
    let mut lat = scale(&lat0, 90, 180)?.to_i64();
    let lon = scale(&lon0, 180, 360)?.to_i64();
    if lat == 0 {
        lat = if pl::lt(&arg(&coords, 0), &n(0)) { 1 } else { 0xfffff }; // (zero latitude is a problem for our calculations)
    }
    let coord = pl::pack("nCn", &[n(lat >> 4), n(((lat & 0x0f) << 4) | (lon & 0x0f)), n(lon >> 4)])?.to_bytes();
    let cities = city_list.items();
    // start from cached city matches if also using city information
    let by_lat = last_by_lat.items();
    let num_entries = if !by_lat.is_empty() { by_lat.len() } else { cities.len() } as i64;
    // binary search to find closest longitude
    let (mut n0, mut n1) = (0i64, num_entries - 1);
    let sort_order = la(it, "sortOrder").items();
    let sorted: Option<&Vec<Value>> = if !by_lat.is_empty() {
        Some(&by_lat)
    } else if !sort_order.is_empty() {
        Some(&sort_order)
    } else {
        None
    };
    let pick = |k: i64| -> Value {
        match sorted {
            Some(s) => s.get(k as usize).cloned().unwrap_or_default(),
            None => n(k),
        }
    };
    let entry_of = |iv: &Value| -> std::rc::Rc<[u8]> { cities.get(iv.to_i64() as usize).map(|v| v.to_bytes()).unwrap_or_else(|| std::rc::Rc::from(&b""[..])) };
    while n1 - n0 > 1 {
        let nn = (n0 + n1) / 2;
        if &coord[..] < &entry_of(&pick(nn))[..] {
            n1 = nn;
        } else {
            n0 = nn;
        }
    }
    // step backward then forward through database to find nearest city
    let (mut inc, mut end, mut nn) = (-1i64, -1i64, n0 + 1);
    let (p0, t0) = (lat as f64 * pi / 0x100000 as f64 - pi / 2.0, lon as f64 * pi / 0x080000 as f64 - pi);
    let cp0 = p0.cos();
    let mut matches: Vec<Vec<Value>> = Vec::new();
    let (mut rtn_list, mut dist) = (Vec::new(), Vec::new());
    let num_f = num.to_f64();

    loop {
        nn += inc;
        if nn == end {
            if inc == 1 || n0 == n1 {
                break;
            }
            (inc, end, nn) = (1, num_entries, n1);
        }
        let iv = pick(nn);
        let entry = entry_of(&iv);
        // get city latitude/longitude
        let g = |k: usize| entry.get(k).copied().unwrap_or(0) as i64;
        let (mut lt, f, mut ln) = ((g(0) << 8) | g(1), g(2), (g(3) << 8) | g(4));
        lt = (lt << 4) | (f >> 4);
        // searched far enough if latitude alone is further than best distance
        if ((lt - lat).abs() as f64) > min_dist_c {
            nn = end - inc;
            continue;
        }
        // ignore if population is below threshold
        if let Some(mp) = &min_pop {
            if &mp[..] >= entry.get(6..8).unwrap_or(&[]) {
                continue;
            }
        }
        if fcodes.truthy() && !fc_test(&entry, &fc_ok) {
            continue;
        }
        ln = (ln << 4) | (f & 0x0f);
        // calculate great circle distance to this city on unit sphere
        let (p1, t1) = (lt as f64 * pi / 0x100000 as f64 - pi / 2.0, ln as f64 * pi / 0x080000 as f64 - pi);
        let (sp, st) = (((p1 - p0) / 2.0).sin(), ((t1 - t0) / 2.0).sin());
        let a_ = sp * sp + cp0 * p1.cos() * st * st;
        let dist_u = a_.sqrt().atan2((1.0 - a_).sqrt()); // distance on unit sphere
        if dist_u > min_dist_u {
            continue;
        }
        match_parms = vec![iv.clone(), Value::Num(p1), Value::Num(t1), Value::Num(dist_u)];
        if num_f <= 1.0 {
            min_dist_u = dist_u;
        } else {
            // add this entry into list of matching cities ordered by closest first
            let mut j = 0usize;
            while j < matches.len() {
                if dist_u < matches[j][3].to_f64() {
                    break;
                }
                j += 1;
            }
            if (j as i64) < matches.len() as i64 - 1 {
                matches.insert(j, match_parms.clone());
            } else if j < matches.len() {
                matches[j] = match_parms.clone();
            } else {
                matches.push(match_parms.clone());
            }
            // restrict list to the specified number of nearest cities
            if matches.len() as f64 > num_f {
                matches.pop();
            }
            // update minimum distance with furthest match if we satisfied our quota
            if matches.len() as f64 >= num_f {
                min_dist_u = matches.last().unwrap()[3].to_f64();
            }
        }
        min_dist_c = min_dist_u * 0x200000 as f64 / pi; // distance in scaled coordinate units
    }
    if match_parms.is_empty() {
        pl::warn(it, "No suitable location in Geolocation database\n")?;
        return pl::ret_nothing(want);
    }
    let num = matches.len();

    if !start_time.is_empty() {
        let t = pl::call(it, "Time::HiRes::tv_interval", vec![Value::new_array(start_time.clone())])?;
        pl::print_to(it, None, &[pl::sprintf("- Processing time: %.3f sec\n", &[t])])?;
    }

    loop {
        if num > 1 {
            if matches.is_empty() {
                break;
            }
            match_parms = matches.remove(0);
        }
        // calculate distance in km and bearing to matching city
        let (ii, p1, t1, dist_u) = (match_parms[0].clone(), match_parms[1].to_f64(), match_parms[2].to_f64(), match_parms[3].to_f64());
        let km = pl::sprintf("%.2f", &[Value::Num(2.0 * earth_radius * dist_u)]);
        let be = ((t1 - t0).sin() * (p1 - p0).cos()).atan2(cp0 * p1.sin() - p0.sin() * p1.cos() * (t1 - t0).cos());
        let be = pl::modulo(&pl::int(&Value::Num(be * 180.0 / pi + 360.5)), &n(360))?; // convert from radians to integer degrees
        rtn_list.push(ii);
        dist.push(Value::new_array(vec![km, be]));
        if num <= 1 {
            break;
        }
    }
    if want == Want::List {
        Ok(vec![Value::new_array(rtn_list), Value::new_array(dist)])
    } else {
        Ok(vec![Value::new_array(rtn_list)])
    }
}

/// The file lexicals ReadDatabase fills: saved before reading a database
/// of the user's, to come back to the built-in one if that read fails.
const DB_ARRAYS: &[&str] = &["cityList", "countryList", "regionList", "subregionList", "timezoneList", "altNames", "featureCodes"];
const DB_HASHES: &[&str] = &["countryNum", "regionNum", "subregionNum", "timezoneNum", "featureTypes", "featureCodes"];
const DB_SCALARS: &[&str] = &["dbVer", "nCity"];

/// Geolocation.pm's module-level code that depends on the run (the module's
/// dump calls it once loaded). The dump holds the database the module read
/// from lib/Image/ExifTool/Geolocation.dat, which stands for that default
/// one here: $geoDir set to "" means no database, and a database in the
/// $geoDir directory replaces it (as perl reads that one first). Then the
/// alternate names directory and the user-defined entries.
fn init_geolocation(it: &mut Interp, _a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let geo_var = format!("{MODULE}::geoDir");
    let geo_dir = it.get_scalar(&geo_var);
    // read the Geolocation database unless $geoDir set to empty string
    if !geo_dir.is_undef() && !geo_dir.truthy() {
        for a in DB_ARRAYS {
            if let Value::ArrayRef(r) = runtime::lex_array(it, MODULE, a) {
                r.borrow_mut().clear();
            }
        }
        for h in DB_HASHES {
            let v = runtime::lex_hash(it, MODULE, h);
            for k in v.keys() {
                v.delete(&k);
            }
        }
        *runtime::lex_scalar(it, MODULE, "nCity").borrow_mut() = Value::Undef;
        it.set_scalar(&format!("{MODULE}::dbInfo"), Value::Undef);
    } else if geo_dir.truthy() {
        let arrays: Vec<Vec<Value>> = DB_ARRAYS.iter().map(|a| runtime::lex_array(it, MODULE, a).items()).collect();
        let hashes: Vec<Vec<(std::rc::Rc<[u8]>, Value)>> = DB_HASHES
            .iter()
            .map(|h| {
                let v = runtime::lex_hash(it, MODULE, h);
                v.keys().into_iter().map(|k| (k.clone(), v.get(&k))).collect()
            })
            .collect();
        let scalars: Vec<Value> = DB_SCALARS.iter().map(|s| runtime::lex_scalar(it, MODULE, s).borrow().clone()).collect();
        let db_info = it.get_scalar(&format!("{MODULE}::dbInfo"));
        // (ReadDatabase starts from empty lists, as the module loading)
        for a in DB_ARRAYS {
            if let Value::ArrayRef(r) = runtime::lex_array(it, MODULE, a) {
                r.borrow_mut().clear();
            }
        }
        let file = format!("{}/Geolocation.dat", geo_dir.to_text());
        let ok = pl::call(it, &format!("{MODULE}::ReadDatabase"), vec![Value::str(&file)])?.truthy();
        if !ok {
            // back to the default database
            for (a, items) in DB_ARRAYS.iter().zip(arrays) {
                if let Value::ArrayRef(r) = runtime::lex_array(it, MODULE, a) {
                    *r.borrow_mut() = items;
                }
            }
            for (h, pairs) in DB_HASHES.iter().zip(hashes) {
                let v = runtime::lex_hash(it, MODULE, h);
                for k in v.keys() {
                    v.delete(&k);
                }
                for (k, x) in pairs {
                    v.set(&k, x);
                }
            }
            for (s, x) in DB_SCALARS.iter().zip(scalars) {
                *runtime::lex_scalar(it, MODULE, s).borrow_mut() = x;
            }
            it.set_scalar(&format!("{MODULE}::dbInfo"), db_info);
        }
    }
    // set directory for language files and alternate names (the built-in
    // database has no directory: $geoDir stays as the user set it)
    let alt_var = format!("{MODULE}::altDir");
    let geo_dir = it.get_scalar(&geo_var);
    if it.get_scalar(&alt_var).is_undef() && geo_dir.truthy() && std::path::Path::new(&format!("{}/AltNames.dat", geo_dir.to_text())).exists() {
        it.set_scalar(&alt_var, geo_dir);
    }
    // add user-defined entries to the database
    if let crate::interp::Binding::Array(a) = it.global(b'@', "Image::ExifTool::UserDefined::Geolocation") {
        let entries: Vec<Value> = a.borrow().clone();
        for e in entries {
            pl::call(it, &format!("{MODULE}::AddEntry"), e.items())?;
        }
    }
    Ok(vec![])
}

pub fn register(it: &mut Interp) {
    it.define_native("Image::ExifTool::Geolocation::InitGeolocation", init_geolocation);
    it.define_native("Image::ExifTool::Geolocation::ReadDatabase", read_database);
    it.define_native("Image::ExifTool::Geolocation::ReadAltNames", read_alt_names);
    it.define_native("Image::ExifTool::Geolocation::ClearLastMatches", clear_last_matches);
    it.define_native("Image::ExifTool::Geolocation::SortDatabase", sort_database);
    it.define_native("Image::ExifTool::Geolocation::AddEntry", add_entry);
    it.define_native("Image::ExifTool::Geolocation::GetEntry", get_entry);
    it.define_native("Image::ExifTool::Geolocation::GetAltNames", get_alt_names);
    it.define_native("Image::ExifTool::Geolocation::Geolocate", geolocate);
}
