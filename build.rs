fn main() {
    #[cfg(all(target_os = "windows", feature = "win-icon"))]
    {
        let mut res = winres::WindowsResource::new();
        res.set_icon("assets/icon.ico");
        res.compile().expect("Failed to compile Windows resources");
    }
}
