//! POSIX::strptime 0.13, as Writer.pl's InverseDateTime calls it (it tries
//! it before Time::Piece) for the inverse date conversions of `-d FMT`:
//! `POSIX::strptime($string, $format)` returns the list (sec, min, hour,
//! mday, mon, year, wday, yday, isdst), each undef when the string did not
//! set it.
//!
//! As the module, this is the C library's strptime(3) (on Unix, called
//! here through libc, with the LC_TIME of the environment, as perl sets it
//! at startup) on a struct tm whose fields start unset (-1, the year
//! INT_MIN); a string that does not match gives the fields it set before
//! the mismatch, and trailing text is ignored. The week day and year day
//! that neither the string nor strptime's own arithmetic set come out as
//! today's (measured against the module on glibc 2.39). Windows has no
//! strptime: there the parser is Time::Piece's (FreeBSD's).

use crate::interp::Interp;
use crate::perl::Value;
use crate::pl::arg;

/// An unset year.
const NO_YEAR: i64 = i32::MIN as i64;

/// The 9 fields for `s` parsed with `fmt` (None: unset).
pub fn strptime(s: &[u8], fmt: &[u8]) -> [Option<i64>; 9] {
    let (f, isdst) = parse(s, fmt);
    let today = crate::interp::time::broken_down(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0), true);
    let mut out = [None; 9];
    for i in 0..8 {
        let unset = if i == 5 { f[i] == NO_YEAR } else { f[i] == -1 };
        out[i] = if !unset {
            Some(f[i])
        } else if i >= 6 {
            Some(today[i])
        } else {
            None
        };
    }
    out[8] = (isdst != -1).then_some(isdst);
    out
}

#[cfg(unix)]
fn parse(s: &[u8], fmt: &[u8]) -> ([i64; 8], i64) {
    use std::ffi::CString;
    // (a NUL ends the C strings, as in the XS's char *)
    let cut = |b: &[u8]| CString::new(b.split(|&c| c == 0).next().unwrap_or_default()).unwrap_or_default();
    let (cs, cf) = (cut(s), cut(fmt));
    // SAFETY: tm is plain data; strptime reads the two NUL-terminated
    // strings and writes the struct; setlocale gets valid locale names and
    // its previous LC_TIME is copied before the next call changes it
    unsafe {
        let mut tm: libc::tm = std::mem::zeroed();
        tm.tm_sec = -1;
        tm.tm_min = -1;
        tm.tm_hour = -1;
        tm.tm_mday = -1;
        tm.tm_mon = -1;
        tm.tm_year = i32::MIN;
        tm.tm_wday = -1;
        tm.tm_yday = -1;
        tm.tm_isdst = -1;
        let prev = libc::setlocale(libc::LC_TIME, std::ptr::null());
        let prev = (!prev.is_null()).then(|| std::ffi::CStr::from_ptr(prev).to_owned());
        libc::setlocale(libc::LC_TIME, c"".as_ptr());
        libc::strptime(cs.as_ptr(), cf.as_ptr(), &mut tm);
        if let Some(p) = prev {
            libc::setlocale(libc::LC_TIME, p.as_ptr());
        }
        let f = [tm.tm_sec, tm.tm_min, tm.tm_hour, tm.tm_mday, tm.tm_mon, tm.tm_year, tm.tm_wday, tm.tm_yday];
        (f.map(i64::from), tm.tm_isdst as i64)
    }
}

#[cfg(not(unix))]
fn parse(s: &[u8], fmt: &[u8]) -> ([i64; 8], i64) {
    (super::time_piece::strptime_fields(s, fmt, [-1, -1, -1, -1, -1, NO_YEAR, -1, -1]), -1)
}

fn native(_: &mut Interp, a: Vec<Value>, _: crate::interp::Want) -> crate::interp::eval::ER<Vec<Value>> {
    let f = strptime(&arg(&a, 0).to_bytes(), &arg(&a, 1).to_bytes());
    Ok(f.iter().map(|v| v.map(Value::Int).unwrap_or(Value::Undef)).collect())
}

pub fn register(it: &mut Interp) {
    it.set_scalar("POSIX::strptime::VERSION", Value::str("0.13"));
    it.define_native("POSIX::strptime::strptime", native);
}

/// `*POSIX::strptime = \&strptime`, as the module does when it loads.
pub fn alias(it: &mut Interp) {
    it.define_native("POSIX::strptime", native);
}

#[cfg(test)]
mod tests {
    use super::strptime;

    /// POSIX::strptime 0.13 on glibc 2.39 (scripts/full_modules), the week
    /// and year days aside where they are today's.
    #[test]
    fn fields_as_the_module() {
        let f = |s: &str, fmt: &str| strptime(s.as_bytes(), fmt.as_bytes());
        assert_eq!(f("2024:05:06 07:08:09", "%Y:%m:%d %H:%M:%S"), [Some(9), Some(8), Some(7), Some(6), Some(4), Some(124), Some(1), Some(126), None]);
        assert_eq!(f("2024/05/06", "%Y/%m/%d")[..8], [None, None, None, Some(6), Some(4), Some(124), Some(1), Some(126)]);
        assert_eq!(f("07:08", "%H:%M")[..6], [None, Some(8), Some(7), None, None, None]);
        assert_eq!(f("May 6 2024 7pm", "%b %d %Y %I%p")[..8], [None, None, Some(19), Some(6), Some(4), Some(124), Some(1), Some(126)]);
        assert_eq!(f("bad", "%Y")[..6], [None; 6]);
        assert_eq!(f("2024:05:06 junk", "%Y:%m:%d")[..6], [None, None, None, Some(6), Some(4), Some(124)]);
        assert_eq!(f("2024-13-01", "%Y-%m-%d")[..6], [None, None, None, None, None, Some(124)]);
        assert_eq!(f("2024 100", "%Y %j")[..8], [None, None, None, Some(9), Some(3), Some(124), Some(2), Some(99)]);
    }
}
