//! Write a tag to a file.
//!
//! Usage: cargo run --example write_tag -- input.jpg output.jpg Artist "John Doe"

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 5 {
        eprintln!("Usage: {} <input> <output> <tag> <value>", args[0]);
        std::process::exit(1);
    }

    let mut et = exiftool::ExifTool::new();
    et.set_new_value(&args[3], Some(&args[4]));
    match et.write_info(&args[1], &args[2]) {
        Ok(count) => println!("{} tag(s) written to {}", count, args[2]),
        Err(e) => eprintln!("Error: {}", e),
    }
}
