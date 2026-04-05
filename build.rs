fn main() {
    // Only embed the Windows icon/version resource when building the standalone
    // binaries (CLI/GUI). Library consumers should NOT get this resource linked
    // into their own binary — use the `win-icon` feature to opt in.
    #[cfg(all(target_os = "windows", feature = "win-icon"))]
    {
        let mut res = winres::WindowsResource::new();
        res.set_icon("assets/icon.ico");
        res.compile().expect("Failed to compile Windows resources");
    }
}
