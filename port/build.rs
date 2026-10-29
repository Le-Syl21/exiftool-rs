fn main() {
    // (not run again for every change of the sources)
    println!("cargo:rerun-if-changed=build.rs");
    // As on main: the Windows icon/version resource, for the executables
    // (exiftool-rs, exiftool-rs-gui) only when asked for with the `win-icon`
    // feature, so that a program using the library does not get it.
    #[cfg(all(target_os = "windows", feature = "win-icon"))]
    {
        let mut res = winres::WindowsResource::new();
        res.set_icon("../assets/icon.ico");
        res.compile().expect("Failed to compile Windows resources");
    }
}
