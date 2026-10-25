//! `localtime` and `gmtime`.

/// perl's 9-element broken-down time: sec, min, hour, mday, mon, year-1900,
/// wday, yday, isdst.
pub fn broken_down(t: i64, local: bool) -> [i64; 9] {
    if local {
        if let Some(tm) = local_tm(t) {
            return tm;
        }
    }
    gm_tm(t)
}

fn gm_tm(t: i64) -> [i64; 9] {
    let days = t.div_euclid(86400);
    let secs = t.rem_euclid(86400);
    let (y, m, d) = civil_from_days(days);
    let yday = days - days_from_civil(y, 1, 1);
    let wday = (days + 4).rem_euclid(7);
    [secs % 60, (secs / 60) % 60, secs / 3600, d, m - 1, y - 1900, wday, yday, 0]
}

#[cfg(unix)]
fn local_tm(t: i64) -> Option<[i64; 9]> {
    let tt = t as libc::time_t;
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    // SAFETY: localtime_r writes into the tm we own.
    let r = unsafe { libc::localtime_r(&tt, &mut tm) };
    if r.is_null() {
        return None;
    }
    Some([tm.tm_sec as i64, tm.tm_min as i64, tm.tm_hour as i64, tm.tm_mday as i64, tm.tm_mon as i64, tm.tm_year as i64, tm.tm_wday as i64, tm.tm_yday as i64, tm.tm_isdst as i64])
}

#[cfg(not(unix))]
fn local_tm(_t: i64) -> Option<[i64; 9]> {
    None
}

/// Days since 1970-01-01 to (year, month, day), proleptic Gregorian.
pub fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719468;
    let era = z.div_euclid(146097);
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { y + 1 } else { y }, m, d)
}

pub fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}

/// The scalar form: "Thu Sep  9 01:46:40 1999".
pub fn ctime(tm: &[i64; 9]) -> String {
    const DAYS: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
    const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
    format!(
        "{} {} {:2} {:02}:{:02}:{:02} {}",
        DAYS[tm[6].rem_euclid(7) as usize],
        MONTHS[tm[4].rem_euclid(12) as usize],
        tm[3],
        tm[2],
        tm[1],
        tm[0],
        tm[5] + 1900
    )
}

/// `getpwuid`/`getgrgid` in scalar context: the user or group name.
#[cfg(unix)]
pub fn user_or_group_name(id: i64, user: bool) -> Option<String> {
    use std::ffi::CStr;
    let mut buf = vec![0 as libc::c_char; 16384];
    // SAFETY: the buffers are ours and sized as declared.
    unsafe {
        if user {
            let mut pw: libc::passwd = std::mem::zeroed();
            let mut out: *mut libc::passwd = std::ptr::null_mut();
            if libc::getpwuid_r(id as libc::uid_t, &mut pw, buf.as_mut_ptr(), buf.len(), &mut out) != 0 || out.is_null() {
                return None;
            }
            Some(CStr::from_ptr(pw.pw_name).to_string_lossy().into_owned())
        } else {
            let mut gr: libc::group = std::mem::zeroed();
            let mut out: *mut libc::group = std::ptr::null_mut();
            if libc::getgrgid_r(id as libc::gid_t, &mut gr, buf.as_mut_ptr(), buf.len(), &mut out) != 0 || out.is_null() {
                return None;
            }
            Some(CStr::from_ptr(gr.gr_name).to_string_lossy().into_owned())
        }
    }
}

#[cfg(not(unix))]
pub fn user_or_group_name(_id: i64, _user: bool) -> Option<String> {
    None
}

/// Seconds since the epoch of a local wall-clock time given as if it were
/// UTC: what `timelocal`/`mktime` compute.
pub fn local_to_utc(wall: f64) -> f64 {
    // The offset at the guessed instant, then once more at the corrected
    // one (DST transitions).
    let offset_at = |t: i64| -> i64 {
        let tm = broken_down(t, true);
        let as_utc = days_from_civil(tm[5] + 1900, tm[4] + 1, tm[3]) * 86400 + tm[2] * 3600 + tm[1] * 60 + tm[0];
        as_utc - t
    };
    let w = wall.floor() as i64;
    let mut t = w - offset_at(w);
    t = w - offset_at(t);
    t as f64 + (wall - wall.floor())
}

/// POSIX::strftime with the C library's formats (the common ones).
pub fn strftime(fmt: &str, tm: &[i64]) -> String {
    const DAYS: [&str; 7] = ["Sunday", "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday"];
    const MONTHS: [&str; 12] = ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"];
    let g = |i: usize| tm.get(i).copied().unwrap_or(0);
    // Normalize like mktime: recompute wday/yday from the date.
    let (year, mon, mday) = (g(5) + 1900, g(4), g(3));
    let days = days_from_civil(year, mon + 1, mday);
    let (y, m, d) = civil_from_days(days);
    let wday = (days + 4).rem_euclid(7);
    let yday = days - days_from_civil(y, 1, 1);
    let (hour, min, sec) = (g(2), g(1), g(0));
    let mut out = String::new();
    let mut chars = fmt.chars();
    while let Some(c) = chars.next() {
        if c != '%' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('Y') => out.push_str(&y.to_string()),
            Some('y') => out.push_str(&format!("{:02}", y.rem_euclid(100))),
            Some('m') => out.push_str(&format!("{:02}", m)),
            Some('d') => out.push_str(&format!("{:02}", d)),
            Some('e') => out.push_str(&format!("{:2}", d)),
            Some('H') => out.push_str(&format!("{:02}", hour)),
            Some('I') => out.push_str(&format!("{:02}", if hour % 12 == 0 { 12 } else { hour % 12 })),
            Some('M') => out.push_str(&format!("{:02}", min)),
            Some('S') => out.push_str(&format!("{:02}", sec)),
            Some('p') => out.push_str(if hour < 12 { "AM" } else { "PM" }),
            Some('j') => out.push_str(&format!("{:03}", yday + 1)),
            Some('a') => out.push_str(&DAYS[wday as usize][..3]),
            Some('A') => out.push_str(DAYS[wday as usize]),
            Some('b') | Some('h') => out.push_str(&MONTHS[(m - 1) as usize][..3]),
            Some('B') => out.push_str(MONTHS[(m - 1) as usize]),
            Some('w') => out.push_str(&wday.to_string()),
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some('%') => out.push('%'),
            Some('z') | Some('Z') | Some('s') => {
                let t = days * 86400 + hour * 3600 + min * 60 + sec;
                let utc = local_to_utc(t as f64) as i64;
                let off = t - utc;
                match c {
                    _ => {}
                }
                out.push_str(&format!("{}{:02}{:02}", if off < 0 { '-' } else { '+' }, off.abs() / 3600, off.abs() % 3600 / 60));
            }
            Some(other) => {
                out.push('%');
                out.push(other);
            }
            None => out.push('%'),
        }
    }
    out
}

/// `utime`: set a file's access and modification times (undef = now).
pub fn set_file_times(path: &str, atime: Option<f64>, mtime: Option<f64>) -> bool {
    let now = std::time::SystemTime::now();
    let t = |x: Option<f64>| x.map(|s| std::time::UNIX_EPOCH + std::time::Duration::from_secs_f64(s.max(0.0))).unwrap_or(now);
    match std::fs::File::options().write(true).open(path).or_else(|_| std::fs::File::open(path)) {
        Ok(f) => f.set_times(std::fs::FileTimes::new().set_accessed(t(atime)).set_modified(t(mtime))).is_ok(),
        Err(_) => false,
    }
}
