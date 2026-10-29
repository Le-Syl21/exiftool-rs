//! exiftool-rs, the exiftool application all in Rust: the port of the script
//! `exiftool` (src/cli) on the built-in library. No Perl file is read
//! (a config file, an argfile or a -if expression the user gives is still
//! Perl, run by the port's interpreter, as ExifTool runs it).
use exiftool_port::interp::{Binding, Flow, Want};
use exiftool_port::perl::Value;

// See the rusty_alloc-api entry in Cargo.toml.
#[global_allocator]
static GLOBAL: rusty_alloc_api::RustyAlloc = rusty_alloc_api::RustyAlloc;

fn main() {
    let mut args = std::env::args();
    let zero = args.next().unwrap_or_else(|| "exiftool".to_string());
    let mut it = exiftool_port::runtime::new_interp();
    it.passthrough = true;
    it.strict = false;
    // $0 is the program, as perl sets it to the script
    it.set_scalar("main::0", Value::str(&zero));
    let argv: Vec<Value> = args.map(|a| Value::str(&a)).collect();
    it.set_global(b'@', "main::ARGV", Binding::Array(std::rc::Rc::new(std::cell::RefCell::new(argv))));
    exiftool_port::cli::register(&mut it);
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
