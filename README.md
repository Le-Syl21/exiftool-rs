# exiftool

A Rust reimplementation of [ExifTool](https://exiftool.org/) — read, write, and edit metadata in image, audio, video, and document files.

## Features

- **41/41 JPEG test files** produce identical tag names as Perl ExifTool v13.52
- **30+ format readers**: JPEG, TIFF, PNG, CR2, PSD, WebP, HEIF/AVIF, MP4/MOV, AVI, MKV, PDF, WAV, FLAC, MP3, OGG, BMP, GIF, DNG, NEF, ARW, ORF, RAF, RW2, PEF, and more
- **15 format writers**: JPEG, TIFF, PNG, WebP, PSD, PDF, MP4, MKV, AVI, WAV, FLAC, MP3, OGG, CR2, HEIF/AVIF
- **15 MakerNote manufacturers**: Canon, Nikon, Sony, Pentax, Olympus, Panasonic, Fujifilm, Samsung, Sigma, Casio, Ricoh, Minolta, Apple, Google, FLIR
- **Specialized parsers**: GoPro GPMF, InfiRay thermal, FlashPix/OLE, Canon VRD, CIFF, MPF, MIE, and more
- **No unsafe code**, minimal dependencies

## Library Usage

```rust
use exiftool::ExifTool;

let et = ExifTool::new();
let tags = et.read_metadata("photo.jpg").unwrap();
for tag in &tags {
    println!("{}: {}", tag.name, tag.print_value);
}
```

## CLI Usage

```bash
# Install
cargo install exiftool

# Read metadata
exiftool photo.jpg

# Short tag names
exiftool -s photo.jpg

# JSON output
exiftool -j photo.jpg

# Write tags
exiftool -Artist="John Doe" -Copyright="2024" photo.jpg

# Show groups
exiftool -G photo.jpg

# Numeric values
exiftool -n photo.jpg
```

## CLI Options

| Option | Description |
|--------|-------------|
| `-s` | Short tag names |
| `-s2` | Very short (tag names only) |
| `-G` | Show group names |
| `-n` | Numeric output |
| `-j` | JSON output |
| `-b` | Binary output |
| `-ver` | Show version |
| `-TAG=VALUE` | Write tag |
| `-overwrite_original` | Overwrite without backup |

## Building

```bash
git clone https://github.com/YOUR_USERNAME/exiftool-rs
cd exiftool-rs
cargo build --release
```

## License

GPL-3.0-or-later (same as the original Perl ExifTool)

## Acknowledgements

Based on [ExifTool](https://exiftool.org/) by Phil Harvey.
Tag tables and print conversions are generated from the ExifTool Perl source.
