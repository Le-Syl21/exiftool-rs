# exiftool-rs

A Rust reimplementation of [ExifTool](https://exiftool.org/) — read, write, and
edit metadata in image, audio, video, and document files.

**38-61x faster** than the original Perl ExifTool. Memory-safe. Single binary.

## Quick Start

### CLI

```bash
# Read metadata
exiftool photo.jpg

# Read specific tags
exiftool -Make -Model -FocalLength photo.jpg

# JSON output
exiftool -j photo.jpg

# Write metadata
exiftool -Artist="John Doe" -Copyright="2024" photo.jpg

# Write EXIF + XMP + IPTC
exiftool -Artist="John" -XMP:Title="My Photo" -IPTC:City="Paris" photo.jpg

# Recursive scan
exiftool -r -ext jpg /photos/

# Copy tags between files
exiftool -tagsFromFile source.jpg destination.jpg

# Shift dates
exiftool -DateTimeOriginal+=5:30:0 photo.jpg

# Conditional processing
exiftool -if '$Make eq "Canon"' -Model *.jpg

# Custom format
exiftool -p '$Make $Model - $FocalLength' *.jpg

# Batch mode (stay-open)
exiftool -stay_open True -@ -
```

### Rust Crate

```rust
use exiftool::ExifTool;

// Read metadata
let et = ExifTool::new();
let info = et.image_info("photo.jpg")?;
for (tag, value) in &info {
    println!("{}: {}", tag, value);
}

// Write metadata
let mut et = ExifTool::new();
et.set_new_value("Artist", Some("John Doe"));
et.set_new_value("XMP:Title", Some("My Photo"));
et.write_info("input.jpg", "output.jpg")?;

// Copy tags from another file
et.set_new_values_from_file("source.jpg", None)?;
et.write_info("input.jpg", "output.jpg")?;
```

## Supported Formats

### Read (29 format readers, 115 file types detected)

| Category | Formats |
|----------|---------|
| **Images** | JPEG, PNG, TIFF, GIF, BMP, WebP, PSD, ICO, JPEG 2000, JPEG XL, HEIF, AVIF, FLIF, BPG, PCX, PICT, DjVu, Radiance HDR, PPM/PGM/PBM |
| **RAW** | CR2, CR3, CRW, NEF, ARW, DNG, ORF, PEF, RW2, RAF, MRW, ERF, SRW, X3F, + 10 more |
| **Video** | MP4, MOV, AVI, MKV, WebM, WMV, FLV, SWF, M2TS, MXF |
| **Audio** | MP3, FLAC, OGG/Opus, AIFF, WAV, AAC, APE |
| **Documents** | PDF, PostScript/EPS, RTF, DOCX, XLSX, PPTX, ODS, HTML |
| **Other** | ZIP, RAR, 7z, GZIP, EXE (PE/ELF/Mach-O), Font (TTF/OTF/WOFF), ICC, DICOM, FITS, JSON |

### Write (11 formats)

JPEG, PNG, TIFF, WebP, MP4/MOV, HEIF/AVIF, MKV/WebM, PSD, PDF, + DNG/CR2/NEF/ARW via TIFF

## Features

- **17,592 print conversions** for human-readable tag values
- **4,286 known tags** across EXIF, IPTC, XMP, and 9 MakerNotes manufacturers
- **Canon MakerNotes sub-tables** decoded (CameraSettings, ShotInfo, FocalLength)
- **Nikon/Sony print conversions** (FlashMode, ShootingMode, DRO, etc.)
- **16 composite tags** (GPSPosition, ImageSize, Megapixels, LightValue, FOV, etc.)
- **Geolocation** reverse geocoding (114,877 cities from ExifTool's database)
- **Stay-open mode** for batch processing
- **5 output formats**: text, JSON, CSV, XML/RDF, tab-separated
- **Conditional filtering** with `-if`
- **Date shifting** with `-DateTimeOriginal+=H:M:S`

## Performance

| Benchmark | Perl ExifTool | exiftool-rs | Speedup |
|-----------|--------------|-------------|---------|
| 193 files (1 invocation) | 2.0s | 54ms | **38x** |
| 193 files (separate) | 25.2s | 0.4s | **61x** |

## Building

```bash
cargo build --release
```

Binary: `target/release/exiftool` (~2.7 MB)

### Regenerate tag tables from Perl source

```bash
# Requires Perl ExifTool source in ../exiftool/
perl scripts/gen_tags.pl ../exiftool/lib > src/tags/generated.rs
perl scripts/gen_print_conv.pl ../exiftool/lib > src/tags/print_conv_generated.rs
```

## Migration from Perl ExifTool

See [MIGRATION.md](MIGRATION.md) for detailed compatibility notes,
output format differences, and a migration checklist.

## License

GPL-3.0-or-later (same as ExifTool)
