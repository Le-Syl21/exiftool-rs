//! Run one of ExifTool's test scripts through the port:
//!   run_t t/BMP.t        (from a directory holding t/, like ExifTool's)
//! Prints the script's output; the exit status is the script's.
use exiftool_port::interp::{Scope, Want};

fn main() {
    let path = std::env::args().nth(1).expect("usage: run_t SCRIPT");
    let src = std::fs::read(&path).unwrap_or_else(|e| panic!("{path}: {e}"));
    let mut it = exiftool_port::runtime::new_interp();
    it.passthrough = true;
    it.strict = false;
    // $0 is the script, as perl sets it
    it.set_scalar("main::0", exiftool_port::perl::Value::str(&path));
    let argv: Vec<exiftool_port::perl::Value> = std::env::args().skip(2).map(|a| exiftool_port::perl::Value::str(&a)).collect();
    it.set_global(b'@', "main::ARGV", exiftool_port::interp::Binding::Array(std::rc::Rc::new(std::cell::RefCell::new(argv))));
    let scope = Scope::new(None);
    it.source_name = Some(std::rc::Rc::from(path.as_str()));
    if let Err(e) = it.eval_string(&src, "main", &scope, Want::Void) {
        if it.exit_code.is_none() {
            eprintln!("died: {}", e.to_text());
            it.exit_code = Some(255);
        }
    }
    let _ = it.run_end_blocks();
    it.flush_handles();
    std::process::exit(it.exit_code.unwrap_or(0));
}
