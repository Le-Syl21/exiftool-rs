# exiftool-rs

A pure Rust reimplementation of [ExifTool](https://exiftool.org/) — read, write, and edit metadata in image, audio, video, and document files.

## Features

- **41/41 JPEG test files** produce identical tag names as Perl ExifTool v13.52
- **30+ format readers**: JPEG, TIFF, PNG, CR2, PSD, WebP, HEIF/AVIF, MP4/MOV, AVI, MKV, PDF, WAV, FLAC, MP3, OGG, BMP, GIF, DNG, NEF, ARW, ORF, RAF, RW2, PEF, and more
- **15 format writers**: JPEG, TIFF, PNG, WebP, PSD, PDF, MP4, MKV, AVI, WAV, FLAC, MP3, OGG, CR2, HEIF/AVIF
- **15 MakerNote manufacturers**: Canon, Nikon, Sony, Pentax, Olympus, Panasonic, Fujifilm, Samsung, Sigma, Casio, Ricoh, Minolta, Apple, Google, FLIR
- **Specialized parsers**: GoPro GPMF, InfiRay thermal, FlashPix/OLE, Canon VRD, CIFF, MPF, MIE, and more
- **No unsafe code**, minimal dependencies

## Library Usage

```rust
use exiftool_rs::ExifTool;

let et = ExifTool::new();
let tags = et.extract_info("photo.jpg").unwrap();
for tag in &tags {
    println!("{}: {}", tag.name, tag.print_value);
}
```

## CLI Usage

```bash
# Install
cargo install exiftool-rs

# Read metadata
exiftool-rs photo.jpg

# Short tag names
exiftool-rs -s photo.jpg

# JSON output
exiftool-rs -j photo.jpg

# Write tags
exiftool-rs -Artist="John Doe" -Copyright="2024" photo.jpg

# Show groups
exiftool-rs -G photo.jpg

# Numeric values
exiftool-rs -n photo.jpg
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

## Testing

```bash
# Unit tests
cargo test

# ISO-functional test (compares against Perl ExifTool)
./scripts/test_iso.sh
```

## Building

```bash
git clone https://github.com/Le-Syl21/exiftool-rs
cd exiftool-rs
cargo build --release
```

## License

GPL-3.0-or-later (same as the original Perl ExifTool)

## Authors

- **Sylvain** ([@Le-Syl21](https://github.com/Le-Syl21)) — Project creator
- **Claude** (Anthropic) — Implementation

## Acknowledgements

Based on [ExifTool](https://exiftool.org/) by Phil Harvey.
Tag tables and print conversions are generated from the ExifTool Perl source.
