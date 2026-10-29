//! File::StatX 0.005 (Linux only), as ExifTool.pm uses it for the
//! FileCreateDate of a file on Linux: `File::StatX::fstatx($fh, 0,
//! STATX_BTIME())` on the open file, or `File::StatX::statx($file, 0,
//! STATX_BTIME())`, then `$stat->btime`, the birth time in seconds with
//! its nanoseconds as a fraction, undef when the filesystem gives none.
//! On the statx(2) system call, through libc.

use std::rc::Rc;

use crate::interp::eval::die;
use crate::interp::Interp;
use crate::perl::Value;
use crate::pl::arg;

/// A File::StatX object from the statx result (or the error).
fn result(r: std::io::Result<libc::statx>) -> crate::interp::eval::ER<Vec<Value>> {
    let st = match r {
        Ok(st) => st,
        Err(e) => return die(format!("Could not statx: {}", crate::interp::eval::io::os_error_text(&e))),
    };
    let h = Value::new_hash();
    if let Value::HashRef(r) = &h {
        r.borrow_mut().blessed = Some(Rc::from("File::StatX"));
    }
    h.set("mask", Value::Int(st.stx_mask as i64));
    if st.stx_mask & libc::STATX_BTIME != 0 {
        h.set("btime", Value::Num(btime(st.stx_btime.tv_sec, st.stx_btime.tv_nsec)));
    }
    Ok(vec![h])
}

/// A statx timestamp in seconds, as the module gives it.
pub fn btime(sec: i64, nsec: u32) -> f64 {
    sec as f64 + nsec as f64 / 1e9
}

/// statx(2) on `path` relative to the working directory, or on `fd` with
/// AT_EMPTY_PATH.
fn statx(fd: Option<i32>, path: &[u8], flags: i64, mask: i64) -> std::io::Result<libc::statx> {
    let p = std::ffi::CString::new(path.split(|&c| c == 0).next().unwrap_or_default()).unwrap_or_default();
    let (dirfd, flags) = match fd {
        Some(fd) => (fd, flags as i32 | libc::AT_EMPTY_PATH),
        None => (libc::AT_FDCWD, flags as i32),
    };
    // SAFETY: statx writes the plain-data struct; the path is NUL-terminated
    unsafe {
        let mut st: libc::statx = std::mem::zeroed();
        if libc::statx(dirfd, p.as_ptr(), flags, mask as u32, &mut st) != 0 {
            return Err(std::io::Error::last_os_error());
        }
        Ok(st)
    }
}

pub fn register(it: &mut Interp) {
    it.set_scalar("File::StatX::VERSION", Value::str("0.005"));
    it.define_native("File::StatX::STATX_BTIME", |_, _, _| Ok(vec![Value::Int(libc::STATX_BTIME as i64)]));
    it.define_native("File::StatX::STATX_BASIC_STATS", |_, _, _| Ok(vec![Value::Int(libc::STATX_BASIC_STATS as i64)]));
    it.define_native("File::StatX::STATX_ALL", |_, _, _| Ok(vec![Value::Int(libc::STATX_ALL as i64)]));
    // statx($path, $flags, $mask)
    it.define_native("File::StatX::statx", |_, a, _| result(statx(None, &arg(&a, 0).to_bytes(), arg(&a, 1).to_i64(), arg(&a, 2).to_i64())));
    // fstatx($fh, $flags, $mask)
    it.define_native("File::StatX::fstatx", |it, a, _| {
        use std::os::fd::AsRawFd;
        let fd = it.with_handle_file(&arg(&a, 0), |f| f.as_raw_fd());
        let r = match fd {
            Some(fd) => statx(Some(fd), b"", arg(&a, 1).to_i64(), arg(&a, 2).to_i64()),
            None => Err(std::io::Error::from_raw_os_error(libc::EBADF)),
        };
        result(r)
    });
    it.define_native("File::StatX::new", |_, _, _| result(Ok(unsafe { std::mem::zeroed() })));
    it.define_native("File::StatX::mask", |_, a, _| Ok(vec![arg(&a, 0).get("mask")]));
    it.define_native("File::StatX::btime", |_, a, _| Ok(vec![arg(&a, 0).get("btime")]));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn birth_time() {
        assert_eq!(btime(1793297733, 935470000), 1793297733.93547);
        // a file's birth time, where the filesystem keeps one, is no later
        // than its modification
        let st = statx(None, b"Cargo.toml", 0, libc::STATX_BTIME as i64).unwrap();
        if st.stx_mask & libc::STATX_BTIME != 0 {
            assert!(st.stx_btime.tv_sec <= std::fs::metadata("Cargo.toml").unwrap().modified().unwrap().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() as i64);
        }
        assert!(statx(None, b"no such file", 0, libc::STATX_BTIME as i64).is_err());
    }
}
