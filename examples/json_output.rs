//! Extract metadata as a HashMap.
//!
//! Usage: cargo run --example json_output -- photo.jpg

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        eprintln!("Usage: {} <file>", args[0]);
        std::process::exit(1);
    }

    let info = exiftool_rs::image_info(&args[1]).unwrap();
    for (key, value) in &info {
        println!("{}: {}", key, value);
    }
}
