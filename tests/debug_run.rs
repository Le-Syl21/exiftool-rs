//! Run a Perl script through the runtime (debugging aid):
//! DEBUG_SCRIPT=path cargo test --test debug_run -- --nocapture
mod common;

#[test]
fn run_script() {
    let Ok(path) = std::env::var("DEBUG_SCRIPT") else { return };
    let Some(root) = common::exiftool_dir("DEBUG_SCRIPT") else { return };
    std::env::set_current_dir(&root).unwrap();
    let src = std::fs::read(&path).unwrap();
    let mut it = exiftool_rs::runtime::new_interp();
    it.passthrough = true;
    it.strict = false;
    let scope = exiftool_rs::interp::Scope::new(None);
    if let Err(e) = it.eval_string(&src, "main", &scope, exiftool_rs::interp::Want::Void) {
        eprintln!("died: {}", e.to_text());
    }
    let _ = it.run_end_blocks();
    it.flush_handles();
}
