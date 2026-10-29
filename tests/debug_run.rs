//! Run a Perl script through the runtime (debugging aid):
//! DEBUG_SCRIPT=path cargo test --test debug_run -- --nocapture
#[test]
fn run_script() {
    let Ok(path) = std::env::var("DEBUG_SCRIPT") else { return };
    let root = format!("{}/../../exiftool", env!("CARGO_MANIFEST_DIR"));
    std::env::set_current_dir(&root).unwrap();
    let src = std::fs::read(&path).unwrap();
    let mut it = exiftool_port::runtime::new_interp();
    it.passthrough = true;
    it.strict = false;
    let scope = exiftool_port::interp::Scope::new(None);
    if let Err(e) = it.eval_string(&src, "main", &scope, exiftool_port::interp::Want::Void) {
        eprintln!("died: {}", e.to_text());
    }
    let _ = it.run_end_blocks();
    it.flush_handles();
}
