//! Read metadata from a file and print all tags.
//!
//! Usage: cargo run --example read_metadata -- photo.jpg

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        eprintln!("Usage: {} <file>", args[0]);
        std::process::exit(1);
    }

    let et = exiftool::ExifTool::new();
    match et.extract_info(&args[1]) {
        Ok(tags) => {
            for tag in &tags {
                println!("{:<32} : {}", tag.name, tag.print_value);
            }
            println!("\n{} tags found.", tags.len());
        }
        Err(e) => eprintln!("Error: {}", e),
    }
}
