//! ExifTool's own test scripts (t/*.t), run unchanged by the interpreter on
//! top of the runtime.

use exiftool_rs::interp::{Scope, Want};

mod common;

/// Run one test script of the ExifTool checkout, in a copy of the files it
/// reads (`files`, under t/), so that the checkout is never written to;
/// returns its output.
fn run_t(root: &std::path::Path, name: &str, files: &[&str]) -> (String, Option<i32>) {
    let work = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("official");
    std::fs::create_dir_all(work.join("t/images")).unwrap();
    for f in [format!("{name}.t"), "TestLib.pm".to_string()].iter().map(String::as_str).chain(files.iter().copied()) {
        std::fs::copy(root.join("t").join(f), work.join("t").join(f)).unwrap();
    }
    std::env::set_current_dir(&work).unwrap();
    let src = std::fs::read(format!("t/{name}.t")).unwrap();
    let mut it = exiftool_rs::runtime::new_interp();
    let scope = Scope::new(None);
    it.strict = false;
    let r = it.eval_string(&src, "main", &scope, Want::Void);
    if let Err(e) = &r {
        if it.exit_code.is_none() {
            it.stderr.extend_from_slice(format!("died: {}", e.to_text()).as_bytes());
        }
    }
    let _ = it.run_end_blocks();
    it.flush_handles();
    let mut out = String::from_utf8_lossy(&it.stdout).into_owned();
    if !it.stderr.is_empty() {
        out.push_str("\n--- stderr ---\n");
        out.push_str(&String::from_utf8_lossy(&it.stderr));
    }
    (out, it.exit_code)
}

fn passed(out: &str) -> bool {
    let plan = out.lines().next().and_then(|l| l.strip_prefix("1..")).and_then(|n| n.trim().parse::<usize>().ok());
    let oks = out.lines().filter(|l| l.starts_with("ok ")).count();
    plan.is_some_and(|n| n == oks) && !out.lines().any(|l| l.starts_with("not ok"))
}

#[test]
fn bmp() {
    let Some(root) = common::exiftool_dir("t/BMP.t") else { return };
    let (out, code) = run_t(&root, "BMP", &["BMP_2.out", "images/BMP.bmp"]);
    assert!(passed(&out) && code.unwrap_or(0) == 0, "t/BMP.t:\n{out}");
}
