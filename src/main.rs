//! exiftool-rs, the exiftool application all in Rust: the port of the script
//! `exiftool` (src/cli) on the built-in library. No Perl file is read
//! (a config file, an argfile or a -if expression the user gives is still
//! Perl, run by the port's interpreter, as ExifTool runs it).
use exiftool_rs::interp::{Binding, Flow, Want};
use exiftool_rs::perl::Value;

// See the rusty_alloc-api entry in Cargo.toml.
#[cfg(feature = "rusty-alloc")]
#[global_allocator]
static GLOBAL: rusty_alloc_api::RustyAlloc = rusty_alloc_api::RustyAlloc;

fn main() {
    // (listed by -ver -v, which the library cannot know)
    #[cfg(feature = "rusty-alloc")]
    exiftool_rs::build_info::set_allocator(exiftool_rs::build_info::crate_desc("rusty_alloc"));
    let mut args = std::env::args();
    let zero = args.next().unwrap_or_else(|| "exiftool".to_string());
    let mut it = exiftool_rs::runtime::new_interp();
    it.passthrough = true;
    it.strict = false;
    // $0 is the program, as perl sets it to the script
    it.set_scalar("main::0", Value::str(&zero));
    let argv: Vec<Value> = args.map(|a| Value::str(&a)).collect();
    it.set_global(b'@', "main::ARGV", Binding::Array(std::rc::Rc::new(std::cell::RefCell::new(argv))));
    exiftool_rs::cli::register(&mut it);
    match it.call_named("main::<program>", vec![], Want::Void) {
        Ok(_) => {}
        Err(Flow::Exit(code)) => it.exit_code = Some(code),
        Err(Flow::Die(e)) => {
            // perl's message for a die that nothing caught
            eprint!("{}", e.to_text());
            it.exit_code = Some(255);
        }
        Err(_) => {}
    }
    let _ = it.run_end_blocks();
    it.flush_handles();
    std::process::exit(it.exit_code.unwrap_or(0));
}
