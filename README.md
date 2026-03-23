# exiftool-rs

[![Crates.io](https://img.shields.io/crates/v/exiftool-rs.svg)](https://crates.io/crates/exiftool-rs)
[![Documentation](https://docs.rs/exiftool-rs/badge.svg)](https://docs.rs/exiftool-rs)
[![License: GPL-3.0](https://img.shields.io/badge/license-GPL--3.0-blue.svg)](LICENSE)

A pure Rust reimplementation of [ExifTool](https://exiftool.org/) — read, write, and edit metadata in image, audio, video, and document files. No unsafe code, no Perl dependency, no system libraries.

![Screenshot](assets/screenshot.png)

## Features

- **194/194 test files (100%)** produce identical tag names as Perl ExifTool v13.53
- **55+ format readers**: JPEG, TIFF, PNG, CR2, CR3, CRW, PSD, WebP, HEIF/AVIF, MP4/MOV, AVI, MKV, MTS, PDF, WAV, FLAC, MP3, OGG, BMP, GIF, DNG, NEF, ARW, ORF, RAF, RW2, PEF, X3F, IIQ, EIP, MIE, MIFF, MRC, DICOM, WTV, DjVu, BPG, XCF, LFP, FPF, and more
- **15 format writers**: JPEG, TIFF, PNG, WebP, PSD, PDF, MP4, MKV, AVI, WAV, FLAC, MP3, OGG, CR2, HEIF/AVIF
- **15+ MakerNote manufacturers**: Canon, Nikon, Sony, Pentax, Olympus, Panasonic, Fujifilm, Samsung, Sigma, Casio, Ricoh, Minolta, Apple, Google, FLIR, GE, GoPro
- **Deep sub-table decoders**: Canon ColorData/CustomFunctions/ShotInfo, Nikon NikonCapture/ScanIFD, Panasonic RW2, Pentax CameraSettings, and more
- **Specialized parsers**: GoPro GPMF, InfiRay thermal, FlashPix/OLE, Canon VRD/CIFF, MPF, MIE, Lytro LFP, FLIR FPF, Sigma X3F, CaptureOne EIP, and more
- **Timed metadata extraction** (`-ee`): GPS and sensor data from dashcams (freeGPS, Kenwood, Novatek, Viofo), action cams (GoPro GPMF, Insta360), drones (DJI, Yuneec), Google CAMM, NMEA
- **Optional GUI** with 23 languages, drag & drop, tag editing, Noto font support for CJK/Arabic/Hindi/Bengali
- **0 compiler warnings**, no unsafe code, minimal dependencies

## Supported Formats

| Category | Read | Write |
|----------|------|-------|
| **Images** | JPEG, TIFF, PNG, WebP, PSD, BMP, GIF, HEIF/AVIF, ICO, PPM, PGF, BPG, XCF, MIFF, PICT | JPEG, TIFF, PNG, WebP, PSD |
| **Raw** | CR2, CR3, CRW, NEF, DNG, ARW, ORF, RAF, RW2, PEF, X3F, IIQ, MRW, 3FR, ERF, SRW | CR2 |
| **Video** | MP4/MOV, AVI, MKV, MTS, WTV, DV, FLV, SWF, MXF, ASF/WMV | MP4, MKV, AVI |
| **Audio** | MP3, FLAC, WAV, OGG, AAC, AIFF, APE, MPC, WavPack, DSF, Audible | MP3, FLAC, WAV, OGG |
| **Documents** | PDF, RTF, HTML, PostScript, DjVu, OpenDocument, TNEF | PDF |
| **Scientific** | DICOM, MRC, FITS, XISF, DPX | — |
| **Archives** | ZIP, RAR, GZIP, ISO, Torrent | — |
| **Other** | EXE/ELF/Mach-O, LNK, VCard, ICS, MIE, Lytro LFP, FLIR FPF, CaptureOne EIP, Palm PDB, PLIST | — |

### MakerNote Support

| Manufacturer | Sub-table Decoders |
|-------------|-------------------|
| **Canon** | CameraSettings, ShotInfo, AFInfo, ColorData (WB), CustomFunctions, VRD, CIFF, CTMD |
| **Nikon** | NikonCapture (D-Lighting, Crop, ColorBoost, UnsharpMask), ScanIFD, CaptureOffsets |
| **Sony** | SonyIDC |
| **Pentax** | CameraSettings (K10D/K-5), AEInfo, LensInfo, FlashInfo, CameraInfo |
| **Olympus** | Equipment, CameraSettings, FocusInfo, RawDevelopment |
| **Panasonic** | RW2 sub-IFDs, AdvancedSceneMode composite |
| **Fujifilm** | RAF WB, PreviewImage |
| **Others** | Samsung, Sigma, Casio, Ricoh, Minolta, Apple, Google, FLIR, GE, GoPro |

### Timed Metadata (`-ee`)

| Source | Formats |
|--------|---------|
| **Dashcams** | freeGPS (Novatek, Viofo, Azdome, Akaso, Vantrue, INNOVV, Nextbase), Kenwood (DRV-A510W) |
| **Action cams** | GoPro GPMF (GPS, accelerometer, gyroscope), Insta360 |
| **Drones** | DJI telemetry, Yuneec/Autel (subtitle tracks) |
| **Android** | Google CAMM (camera motion metadata, types 0-7) |
| **Generic** | NMEA sentences (RMC, GGA), RVMI, Garmin |

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
# Install (CLI only)
cargo install exiftool-rs

# Install with GUI
cargo install exiftool-rs --features gui

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

# Extract embedded GPS from dashcam video
exiftool-rs -ee video.mp4
```

## GUI

exiftool-rs includes an optional graphical interface built with [egui](https://github.com/emilk/egui).

```bash
# Install with GUI support
cargo install exiftool-rs --features gui

# Launch the GUI
exiftool-rs-gui

# Or with a specific language
exiftool-rs-gui -lang fr
```

**Features:**
- Open files or folders, navigate with arrow keys
- View all metadata grouped by category
- Double-click any writable tag to edit it
- Copy all metadata to clipboard
- Save edits back to the file
- 23 languages supported (CJK, Arabic, Hindi, Bengali fonts included via [noto-fonts-dl](https://crates.io/crates/noto-fonts-dl))

### Supported Languages

| Code | Language | Code | Language |
|------|----------|------|----------|
| `en` | English | `ko` | Korean |
| `ar` | Arabic | `nl` | Dutch |
| `bn` | Bengali | `pl` | Polish |
| `cs` | Czech | `pt` | Portuguese |
| `de` | German | `ru` | Russian |
| `en_ca` | English (Canada) | `sk` | Slovak |
| `en_gb` | English (UK) | `sv` | Swedish |
| `es` | Spanish | `tr` | Turkish |
| `fi` | Finnish | `zh` | Chinese (Simplified) |
| `fr` | French | `zh_tw` | Chinese (Traditional) |
| `hi` | Hindi | | |
| `it` | Italian | | |
| `ja` | Japanese | | |

### Platform Notes

| OS | Icon in binary | Notes |
|----|---------------|-------|
| **Windows** | Yes | Icon embedded in `.exe` via `winres` |
| **macOS** | No | Requires a `.app` bundle with `.icns` |
| **Linux** | No | ELF binaries don't support embedded icons |

## CLI Options

| Option | Description |
|--------|-------------|
| `-s` | Short tag names |
| `-s2` | Very short (tag names only) |
| `-G` | Show group names |
| `-n` | Numeric output (no print conversion) |
| `-j` | JSON output |
| `-csv` | CSV output |
| `-X` | XML/RDF output |
| `-b` | Binary output (thumbnails, etc.) |
| `-ee` | Extract embedded data (GPS from dashcams, etc.) |
| `-r` | Recursively scan directories |
| `-ext EXT` | Process only files with extension EXT |
| `-TAG` | Extract specific tag(s) |
| `-ver` | Show version |
| `-TAG=VALUE` | Write tag |
| `-TAG=` | Delete tag |
| `-overwrite_original` | Overwrite without backup |
| `-stay_open True` | Keep running, read commands from stdin |

## Testing

```bash
# Unit tests
cargo test

# ISO-functional test against Perl ExifTool's 194 test files
# No Perl required — reference files included in tests/expected/
./scripts/test_iso.sh

# 194/194 files (100%) produce identical tag names — 11625 tags verified
```

## Building

```bash
git clone https://github.com/Le-Syl21/exiftool-rs
cd exiftool-rs

# CLI only
cargo build --release

# CLI + GUI
cargo build --release --features gui
```

The `gui` feature is optional and not included by default. Without it, the GUI dependencies (egui, eframe, image, rfd, noto-fonts-dl) are not downloaded or compiled.

## License

GPL-3.0-or-later (same as the original Perl ExifTool)

## Authors

- **Sylvain** ([@Le-Syl21](https://github.com/Le-Syl21)) — Project creator
- **Claude** (Anthropic) — Implementation

## Acknowledgements

Based on [ExifTool](https://exiftool.org/) by Phil Harvey.
Tag tables and print conversions are generated from the ExifTool Perl source.
