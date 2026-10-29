//! The Windows modules ExifTool and the exiftool script load on MSWin32,
//! the parts they use, on the Win32 API through windows-sys:
//!
//! - Win32::API: `Win32::API->new('KERNEL32', NAME, PROTO, RET)` and
//!   `->Call(...)` for the four functions ExifTool imports with it:
//!   GetFullPathNameW (WindowsLongPath), CreateDirectoryW (directories with
//!   Unicode names), GetFileTime and SetFileTime (the file times, with
//!   FileCreateDate, and -P). A 'P' argument is a string buffer that the
//!   call writes into, in place, as Win32::API does; a number 0 is a NULL
//!   pointer. Other functions are not imported (new returns undef).
//! - Win32API::File: CreateFileW, OsFHandleOpenFd and CloseHandle (Open
//!   and Exists on wide names), GetOsFHandle (the handle of a perl file for
//!   the file times), GetFileAttributes(W), DeleteFile(W), MoveFileExW, and
//!   the constants ExifTool names. A wide name is UTF-16LE, as ExifTool's
//!   EncodeFileName makes it (with its two NUL bytes).
//! - Win32::FindFile: FindFile (wildcards) and ReadDir (directories), whose
//!   entries' cFileName is the name in UTF-8.
//! - Win32::GetLastError, the error of the last of these calls.
//!
//! OsFHandleOpenFd gives a descriptor number that `open(FH, "<&=FD")` then
//! takes over (the interpreter's files are Rust files, not C runtime
//! descriptors), so that a file opened by its wide name reads and writes
//! as any other.
//!
//! What does not depend on the platform (the conversions) is below, and
//! tested anywhere; the calls themselves are Windows only.

/// The UTF-16 code units of a wide string argument: its UTF-16LE bytes up
/// to the first NUL character, with one NUL after them.
pub fn wide(b: &[u8]) -> Vec<u16> {
    let mut w: Vec<u16> = b.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).take_while(|&c| c != 0).collect();
    w.push(0);
    w
}

/// The bytes of an ANSI string argument up to the first NUL, with a NUL.
pub fn ansi(b: &[u8]) -> Vec<u8> {
    let mut a: Vec<u8> = b.iter().copied().take_while(|&c| c != 0).collect();
    a.push(0);
    a
}

/// UTF-16 code units (up to a NUL) as UTF-8, and the first unpaired
/// surrogate if there is one (written as U+FFFD).
pub fn utf8_name(w: &[u16]) -> (Vec<u8>, Option<u16>) {
    let end = w.iter().position(|&c| c == 0).unwrap_or(w.len());
    let mut bad = None;
    let s: String = char::decode_utf16(w[..end].iter().copied())
        .map(|r| {
            r.unwrap_or_else(|e| {
                bad.get_or_insert(e.unpaired_surrogate());
                '\u{fffd}'
            })
        })
        .collect();
    (s.into_bytes(), bad)
}

/// A FILETIME struct (`pack 'LL'`) as a 64-bit count, or None for a NULL
/// pointer (the number 0, or anything shorter than the struct).
pub fn filetime(b: &[u8]) -> Option<u64> {
    (b.len() >= 8).then(|| u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as u64 | (u32::from_le_bytes([b[4], b[5], b[6], b[7]]) as u64) << 32)
}

/// A FILETIME as its packed struct.
pub fn pack_filetime(t: u64) -> Vec<u8> {
    let mut v = (t as u32).to_le_bytes().to_vec();
    v.extend_from_slice(&((t >> 32) as u32).to_le_bytes());
    v
}

#[cfg(windows)]
mod imp {
    use std::cell::Cell;
    use std::os::windows::io::{AsRawHandle, FromRawHandle, RawHandle};
    use std::rc::Rc;

    use windows_sys::Win32::Foundation::{CloseHandle, GetLastError, FILETIME, GENERIC_READ, GENERIC_WRITE, HANDLE, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::Storage::FileSystem as fs;

    use super::*;
    use crate::interp::eval::ER;
    use crate::interp::Interp;
    use crate::perl::Value;
    use crate::pl::arg;

    thread_local! {
        /// GetLastError() after the last call that failed.
        static LAST_ERROR: Cell<u32> = const { Cell::new(0) };
    }

    fn note_error() {
        // SAFETY: GetLastError only reads the thread's error
        LAST_ERROR.with(|e| e.set(unsafe { GetLastError() }));
    }

    fn handle(v: &Value) -> HANDLE {
        v.to_i64() as isize as HANDLE
    }

    fn handle_value(h: HANDLE) -> Value {
        Value::Int(h as isize as i64)
    }

    fn blessed(class: &str) -> Value {
        let h = Value::new_hash();
        if let Value::HashRef(r) = &h {
            r.borrow_mut().blessed = Some(Rc::from(class));
        }
        h
    }

    /// A 'P' argument: the bytes of its buffer, None for a NULL pointer
    /// (undef or a number 0).
    fn pointer(v: &Value) -> Option<Vec<u8>> {
        match v {
            Value::Undef => None,
            Value::Int(0) => None,
            Value::Num(x) if *x == 0.0 => None,
            v => Some(v.to_bytes().to_vec()),
        }
    }

    const FUNCTIONS: &[&str] = &["GetFullPathNameW", "CreateDirectoryW", "GetFileTime", "SetFileTime"];

    /// Win32::API->new(LIB, NAME, PROTO, RET)
    fn api_new(_: &mut Interp, a: Vec<Value>, _: crate::interp::Want) -> ER<Vec<Value>> {
        let (lib, name) = (arg(&a, 1).to_text(), arg(&a, 2).to_text());
        if !lib.eq_ignore_ascii_case("kernel32") || !FUNCTIONS.contains(&name.as_str()) {
            return Ok(vec![Value::Undef]);
        }
        let o = blessed("Win32::API");
        o.set("procname", Value::str(&name));
        o.set("in", arg(&a, 3));
        o.set("out", arg(&a, 4));
        Ok(vec![o])
    }

    /// $api->Call(ARGS...)
    fn api_call(it: &mut Interp, a: Vec<Value>, _: crate::interp::Want) -> ER<Vec<Value>> {
        let name = arg(&a, 0).get("procname").to_text();
        // (the arguments after the object, at @_ index i + 1)
        let p = |i: usize| arg(&a, i + 1);
        // SAFETY (each call): the strings are NUL-terminated copies, the
        // buffers are as long as said, the handles are the caller's
        let r: i64 = unsafe {
            match name.as_str() {
                "GetFullPathNameW" => {
                    let path = wide(&p(0).to_bytes());
                    let len = p(1).to_i64().max(0) as u32;
                    match pointer(&p(2)) {
                        Some(buf) => {
                            // (the buffer's length in characters bounds what
                            // the call may write)
                            let len = len.min((buf.len() / 2) as u32);
                            let mut out = vec![0u16; len as usize];
                            let n = fs::GetFullPathNameW(path.as_ptr(), len, out.as_mut_ptr(), std::ptr::null_mut());
                            let mut bytes = buf.clone();
                            for (i, c) in out.iter().enumerate() {
                                bytes[2 * i..2 * i + 2].copy_from_slice(&c.to_le_bytes());
                            }
                            it.set_arg(3, Value::bytes(bytes))?;
                            n as i64
                        }
                        None => fs::GetFullPathNameW(path.as_ptr(), 0, std::ptr::null_mut(), std::ptr::null_mut()) as i64,
                    }
                }
                "CreateDirectoryW" => fs::CreateDirectoryW(wide(&p(0).to_bytes()).as_ptr(), std::ptr::null()) as i64,
                "GetFileTime" => {
                    let mut t = [FILETIME { dwLowDateTime: 0, dwHighDateTime: 0 }; 3];
                    let ok = fs::GetFileTime(handle(&p(0)), &mut t[0], &mut t[1], &mut t[2]);
                    if ok != 0 {
                        for (i, ft) in t.iter().enumerate() {
                            // (only the structs the caller passed)
                            if pointer(&p(i + 1)).is_some() {
                                let v = (ft.dwHighDateTime as u64) << 32 | ft.dwLowDateTime as u64;
                                it.set_arg(i + 2, Value::bytes(pack_filetime(v)))?;
                            }
                        }
                    }
                    ok as i64
                }
                "SetFileTime" => {
                    let t: Vec<Option<FILETIME>> = (1..4)
                        .map(|i| pointer(&p(i)).and_then(|b| filetime(&b)).map(|v| FILETIME { dwLowDateTime: v as u32, dwHighDateTime: (v >> 32) as u32 }))
                        .collect();
                    let ptr = |o: &Option<FILETIME>| o.as_ref().map_or(std::ptr::null(), |f| f as *const FILETIME);
                    fs::SetFileTime(handle(&p(0)), ptr(&t[0]), ptr(&t[1]), ptr(&t[2])) as i64
                }
                _ => 0,
            }
        };
        if r == 0 {
            note_error();
        }
        Ok(vec![Value::Int(r)])
    }

    /// A BOOL result, noting the error of a failure.
    fn bool_result(ok: i32) -> ER<Vec<Value>> {
        if ok == 0 {
            note_error();
        }
        Ok(vec![Value::from(ok != 0)])
    }

    /// A Win32::FindFile entry.
    fn entry(d: &fs::WIN32_FIND_DATAW, it: &mut Interp) -> ER<Value> {
        let (name, bad) = utf8_name(&d.cFileName);
        if let Some(c) = bad {
            crate::pl::warn(it, &format!("Unicode surrogate U+{c:04X} is illegal in UTF-8"))?;
        }
        let o = blessed("Win32::FindFile");
        o.set("cFileName", Value::bytes(name));
        o.set("dosName", Value::bytes(utf8_name(&d.cAlternateFileName).0));
        o.set("dwFileAttributes", Value::Int(d.dwFileAttributes as i64));
        o.set("FileSize", Value::Num((d.nFileSizeHigh as u64 * 4294967296 + d.nFileSizeLow as u64) as f64));
        Ok(o)
    }

    /// The entries FindFirstFileW / FindNextFileW give for a pattern.
    fn find(it: &mut Interp, pattern: &[u8]) -> ER<Vec<Value>> {
        let w: Vec<u16> = String::from_utf8_lossy(pattern).encode_utf16().chain(std::iter::once(0)).collect();
        let mut out = Vec::new();
        // SAFETY: the pattern is NUL-terminated; d is plain data the calls fill
        unsafe {
            let mut d: fs::WIN32_FIND_DATAW = std::mem::zeroed();
            let h = fs::FindFirstFileW(w.as_ptr(), &mut d);
            if h == INVALID_HANDLE_VALUE {
                note_error();
                return Ok(out);
            }
            loop {
                out.push(entry(&d, it)?);
                if fs::FindNextFileW(h, &mut d) == 0 {
                    break;
                }
            }
            fs::FindClose(h);
        }
        Ok(out)
    }

    pub fn register(it: &mut Interp) {
        it.set_scalar("Win32::API::VERSION", Value::str("0.84"));
        it.set_scalar("Win32API::File::VERSION", Value::str("0.1203"));
        it.set_scalar("Win32::FindFile::VERSION", Value::str("0.15"));
        it.define_native("Win32::API::new", api_new);
        it.define_native("Win32::API::Call", api_call);
        it.define_native("Win32::GetLastError", |_, _, _| Ok(vec![Value::Int(LAST_ERROR.with(|e| e.get()) as i64)]));

        // the constants
        for (name, v) in [
            ("GENERIC_READ", GENERIC_READ),
            ("GENERIC_WRITE", GENERIC_WRITE),
            ("FILE_APPEND_DATA", fs::FILE_APPEND_DATA),
            ("OPEN_ALWAYS", fs::OPEN_ALWAYS),
            ("CREATE_ALWAYS", fs::CREATE_ALWAYS),
            ("OPEN_EXISTING", fs::OPEN_EXISTING),
            ("FILE_SHARE_READ", fs::FILE_SHARE_READ),
            ("FILE_SHARE_WRITE", fs::FILE_SHARE_WRITE),
            ("INVALID_FILE_ATTRIBUTES", fs::INVALID_FILE_ATTRIBUTES),
            ("FILE_ATTRIBUTE_READONLY", fs::FILE_ATTRIBUTE_READONLY),
            ("FILE_ATTRIBUTE_HIDDEN", fs::FILE_ATTRIBUTE_HIDDEN),
            ("FILE_ATTRIBUTE_DIRECTORY", fs::FILE_ATTRIBUTE_DIRECTORY),
            ("MOVEFILE_REPLACE_EXISTING", fs::MOVEFILE_REPLACE_EXISTING),
            ("MOVEFILE_COPY_ALLOWED", fs::MOVEFILE_COPY_ALLOWED),
        ] {
            it.define_native(&format!("Win32API::File::{name}"), move |_, _, _| Ok(vec![Value::Int(v as i64)]));
        }
        // CreateFileW($sPath, $uAccess, $uShare, $pSecAttr, $uCreate,
        // $uFlags, $hModel): the handle, false on failure
        it.define_native("Win32API::File::CreateFileW", |_, a, _| {
            let path = wide(&arg(&a, 0).to_bytes());
            // SAFETY: the path is NUL-terminated; no security attributes or
            // template file
            let h = unsafe { fs::CreateFileW(path.as_ptr(), arg(&a, 1).to_i64() as u32, arg(&a, 2).to_i64() as u32, std::ptr::null(), arg(&a, 4).to_i64() as u32, arg(&a, 5).to_i64() as u32, std::ptr::null_mut()) };
            if h == INVALID_HANDLE_VALUE {
                note_error();
                return Ok(vec![Value::from(false)]);
            }
            Ok(vec![handle_value(h)])
        });
        // OsFHandleOpenFd($hOsFHandle, $uMode): a descriptor for `open
        // "&=FD"`, which then owns the handle
        it.define_native("Win32API::File::OsFHandleOpenFd", |_, a, _| {
            let h = handle(&arg(&a, 0));
            if h.is_null() || h == INVALID_HANDLE_VALUE {
                return Ok(vec![Value::Int(-1)]);
            }
            // SAFETY: the handle is a file handle CreateFileW returned,
            // which the caller gives up (it closes it only on failure)
            let f = unsafe { std::fs::File::from_raw_handle(h as RawHandle) };
            Ok(vec![Value::Int(crate::interp::eval::io::register_os_file(f))])
        });
        it.define_native("Win32API::File::CloseHandle", |_, a, _| {
            // SAFETY: the caller's handle, closed once
            bool_result(unsafe { CloseHandle(handle(&arg(&a, 0))) })
        });
        // GetOsFHandle(FH): the handle of a perl file, false if none
        it.define_native("Win32API::File::GetOsFHandle", |it, a, _| {
            match it.with_handle_file(&arg(&a, 0), |f| f.as_raw_handle() as isize as i64) {
                Some(h) => Ok(vec![Value::Int(h)]),
                None => Ok(vec![Value::from(false)]),
            }
        });
        it.define_native("Win32API::File::GetFileAttributesW", |_, a, _| {
            // SAFETY: a NUL-terminated wide path
            let r = unsafe { fs::GetFileAttributesW(wide(&arg(&a, 0).to_bytes()).as_ptr()) };
            if r == fs::INVALID_FILE_ATTRIBUTES {
                note_error();
            }
            Ok(vec![Value::Int(r as i64)])
        });
        it.define_native("Win32API::File::GetFileAttributes", |_, a, _| {
            // SAFETY: a NUL-terminated ANSI path
            let r = unsafe { fs::GetFileAttributesA(ansi(&arg(&a, 0).to_bytes()).as_ptr()) };
            if r == fs::INVALID_FILE_ATTRIBUTES {
                note_error();
            }
            Ok(vec![Value::Int(r as i64)])
        });
        // SAFETY (the three below): NUL-terminated paths
        it.define_native("Win32API::File::DeleteFileW", |_, a, _| bool_result(unsafe { fs::DeleteFileW(wide(&arg(&a, 0).to_bytes()).as_ptr()) }));
        it.define_native("Win32API::File::DeleteFile", |_, a, _| bool_result(unsafe { fs::DeleteFileA(ansi(&arg(&a, 0).to_bytes()).as_ptr()) }));
        it.define_native("Win32API::File::MoveFileExW", |_, a, _| bool_result(unsafe { fs::MoveFileExW(wide(&arg(&a, 0).to_bytes()).as_ptr(), wide(&arg(&a, 1).to_bytes()).as_ptr(), arg(&a, 2).to_i64() as u32) }));

        // Win32::FindFile::FindFile($pattern) and ReadDir($dir), names in UTF-8
        it.define_native("Win32::FindFile::FindFile", |it, a, _| find(it, &arg(&a, 0).to_bytes()));
        it.define_native("Win32::FindFile::ReadDir", |it, a, _| {
            let mut d = arg(&a, 0).to_bytes().to_vec();
            if !d.is_empty() && !matches!(d.last(), Some(b'/' | b'\\')) {
                d.push(b'\\');
            }
            d.push(b'*');
            find(it, &d)
        });
        for m in ["cFileName", "dosName", "dwFileAttributes", "FileSize"] {
            it.define_native(&format!("Win32::FindFile::{m}"), move |_, a, _| Ok(vec![arg(&a, 0).get(m)]));
        }
        // (not "." or "..")
        it.define_native("Win32::FindFile::is_entry", |_, a, _| {
            let n = arg(&a, 0).get("cFileName").to_bytes();
            Ok(vec![Value::from(&n[..] != b"." && &n[..] != b"..")])
        });
    }
}

#[cfg(windows)]
pub use imp::register;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wide_strings() {
        // ExifTool's UTF-16LE with its two NUL bytes, and without
        assert_eq!(wide(b"a\0\xe9\0\0\0"), vec![0x61, 0xe9, 0]);
        assert_eq!(wide(b"a\0b\0"), vec![0x61, 0x62, 0]);
        // a buffer of GetFullPathNameW, NULs after the path
        assert_eq!(wide(b"c\0:\0\0\0\0\0"), vec![0x63, 0x3a, 0]);
        assert_eq!(ansi(b"ab\0cd"), b"ab\0".to_vec());
    }

    #[test]
    fn names_in_utf8() {
        assert_eq!(utf8_name(&[0x63, 0xe9, 0x4e2d, 0xd83d, 0xde00, 0, 0x41]), ("cé中😀".as_bytes().to_vec(), None));
        assert_eq!(utf8_name(&[0x61, 0xd800, 0x62]), ("a\u{fffd}b".as_bytes().to_vec(), Some(0xd800)));
    }

    #[test]
    fn filetimes() {
        // pack 'LL', lo, hi
        let t = 133_000_000_000_000_000u64;
        assert_eq!(filetime(&pack_filetime(t)), Some(t));
        assert_eq!(filetime(b"0"), None);
    }
}
