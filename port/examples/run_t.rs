//! Run one of ExifTool's test scripts through the port:
//!   run_t t/BMP.t        (from a directory holding t/, like ExifTool's)
//! Prints the script's output; the exit status is the script's.
use exiftool_port::interp::{Scope, Want};

// See the rusty_alloc-api entry in Cargo.toml.
#[global_allocator]
static GLOBAL: rusty_alloc_api::RustyAlloc = rusty_alloc_api::RustyAlloc;

fn main() {
    // (listed by -ver -v, which the library cannot know)
    exiftool_port::build_info::set_allocator(exiftool_port::build_info::crate_desc("rusty_alloc"));
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
    // the program's file lexicals are also main::<my>::NAME, for the natives
    // that port its subs (the exiftool script's)
    it.program_scope = Some((std::rc::Rc::as_ptr(&scope), std::rc::Rc::from("main")));
    // the exiftool application: its ported subs replace the script's
    if path.rsplit('/').next() == Some("exiftool") {
        exiftool_port::cli::register(&mut it);
    }
    it.source_name = Some(std::rc::Rc::from(path.as_str()));
    // the ported main program in place of the script's top-level code
    // (EXIFTOOL_NATIVE_MAIN=1 while it is being checked)
    let native_main = std::env::var_os("EXIFTOOL_NATIVE_MAIN").is_some() && it.sub("main::<program>").is_some();
    let r = if native_main {
        match it.call_named("main::<program>", vec![], Want::Void) {
            Ok(_) => Ok(vec![]),
            Err(exiftool_port::interp::Flow::Exit(code)) => {
                it.exit_code = Some(code);
                Ok(vec![])
            }
            Err(exiftool_port::interp::Flow::Die(v)) => Err(v),
            Err(_) => Ok(vec![]),
        }
    } else {
        it.eval_string(&src, "main", &scope, Want::Void)
    };
    if let Err(e) = r {
        if it.exit_code.is_none() {
            eprintln!("died: {}", e.to_text());
            it.exit_code = Some(255);
        }
    }
    let _ = it.run_end_blocks();
    it.flush_handles();
    std::process::exit(it.exit_code.unwrap_or(0));
}
