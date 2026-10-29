//! What the port is built with, for `exiftool-rs -ver -v`: the compiler and
//! target, the crates' versions (from Cargo.lock, by build.rs), and the
//! versions of the two C libraries in the build, asked of the libraries
//! themselves.

use std::sync::OnceLock;

include!(concat!(env!("OUT_DIR"), "/build_info.rs"));

/// The version of a crate in the build, as Cargo.lock has it.
pub fn crate_version(name: &str) -> Option<&'static str> {
    DEPS.iter().find(|(n, _)| *n == name).map(|(_, v)| *v)
}

/// `name version` of a crate, or the name alone when it is not in the
/// build's Cargo.lock (the library built in another workspace without one).
pub fn crate_desc(name: &str) -> String {
    match crate_version(name) {
        Some(v) => format!("{name} {v}"),
        None => name.to_string(),
    }
}

/// The PCRE2 library's version ("10.46"), from PCRE2_CONFIG_VERSION.
pub fn pcre2_version() -> String {
    let mut buf = [0u8; 64];
    // SAFETY: the buffer is larger than the version string (24 code units
    // at most), as pcre2_config requires.
    let n = unsafe { pcre2_sys::pcre2_config_8(pcre2_sys::PCRE2_CONFIG_VERSION, buf.as_mut_ptr().cast()) };
    let s = if n > 0 { String::from_utf8_lossy(&buf[..(n as usize - 1).min(buf.len())]).into_owned() } else { String::new() };
    // "10.46 2025-08-27": the version without its date
    s.split_whitespace().next().unwrap_or("unknown").to_string()
}

/// zlib's version ("1.3.2"), from zlibVersion().
pub fn zlib_version() -> String {
    // SAFETY: zlibVersion returns a static NUL-terminated string.
    unsafe { std::ffi::CStr::from_ptr(libz_sys::zlibVersion()) }.to_string_lossy().into_owned()
}

/// How a C library is linked: "static" when its function `f` is in the
/// executable itself, "shared" when it is in a shared library (a system
/// library found by the crate's build script).
pub fn linkage(f: *const std::ffi::c_void) -> &'static str {
    #[cfg(unix)]
    {
        fn base(addr: *const std::ffi::c_void) -> Option<*mut std::ffi::c_void> {
            // SAFETY: dladdr only reads the address; info is plain data.
            let mut info: libc::Dl_info = unsafe { std::mem::zeroed() };
            (unsafe { libc::dladdr(addr, &mut info) } != 0).then_some(info.dli_fbase)
        }
        let own = base(linkage as *const std::ffi::c_void);
        if let (Some(own), Some(lib)) = (own, base(f)) {
            if own != lib {
                return "shared";
            }
        }
    }
    let _ = f;
    "static"
}

/// The PCRE2 library's linkage.
pub fn pcre2_linkage() -> &'static str {
    linkage(pcre2_sys::pcre2_config_8 as *const std::ffi::c_void)
}

/// zlib's linkage.
pub fn zlib_linkage() -> &'static str {
    linkage(libz_sys::zlibVersion as *const std::ffi::c_void)
}

static ALLOCATOR: OnceLock<String> = OnceLock::new();

/// The program's global allocator ("rusty_alloc 2.2.7"), which only the
/// program knows: it is listed by -ver -v when the program sets it.
pub fn set_allocator(desc: impl Into<String>) {
    let _ = ALLOCATOR.set(desc.into());
}

pub fn allocator() -> Option<&'static str> {
    ALLOCATOR.get().map(String::as_str)
}

/// The Perl libraries the port provides itself, as -ver -v lists them in
/// place of perl's optional libraries: (name, what implements it).
pub fn included_libraries() -> Vec<(&'static str, String)> {
    let own = "own Rust implementation";
    let on = |crates: &[&str]| format!("{own}, based on {}", crates.iter().map(|c| crate_desc(c)).collect::<Vec<_>>().join(", "));
    let zlib = format!("{} + {} (zlib {}, C, {})", on(&["flate2"]), crate_desc("libz-sys"), zlib_version(), zlib_linkage());
    let brotli = on(&["mbrotli"]);
    let mut v = vec![
        ("Perl interpreter", format!("{own} (perl 5.38 semantics)")),
        ("Perl regular expressions", format!("{} (PCRE2 {}, C, {})", on(&["pcre2"]), pcre2_version(), pcre2_linkage())),
        ("Compress::Zlib", zlib.clone()),
        ("IO::Compress::RawDeflate", zlib.clone()),
        ("IO::Uncompress::RawInflate", zlib),
        ("IO::Compress::Bzip2", format!("{} ({})", on(&["bzip2"]), crate_desc("libbz2-rs-sys"))),
        ("IO::Compress::Brotli", brotli.clone()),
        ("IO::Uncompress::Brotli", brotli),
        ("Digest::MD5", on(&["md-5"])),
        ("Digest::SHA", on(&["sha1", "sha2"])),
        ("Archive::Zip", own.to_string()),
        // (the C library's strptime, as the module's; Windows has none,
        // and there it is Time::Piece's parser)
        ("POSIX::strptime", if cfg!(unix) { format!("{} (the C library's strptime)", on(&["libc"])) } else { own.to_string() }),
        ("Time::Piece", own.to_string()),
        ("Time::Local", own.to_string()),
        ("Compress::Raw::Lzma", own.to_string()),
        ("Unicode::LineBreak", on(&["unicode-width"])),
    ];
    // the modules of one platform, as the script lists them: File::StatX
    // on Linux, the Win32 ones on Windows
    if cfg!(target_os = "linux") {
        v.push(("File::StatX", format!("{} (statx)", on(&["libc"]))));
    }
    if cfg!(windows) {
        for m in ["Win32::API", "Win32::FindFile", "Win32API::File"] {
            v.push((m, on(&["windows-sys"])));
        }
    }
    if let Some(a) = allocator() {
        v.push(("Memory allocator", a.to_string()));
    }
    v
}
