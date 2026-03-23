# CLAUDE.md - Project Guide for AI Assistants

## Project Overview

exiftool-rs is a Rust reimplementation of Perl ExifTool v13.53.
The Perl source is at `../exiftool/` for reference.

## Architecture

```
src/
├── lib.rs              # Crate root, re-exports
├── main.rs             # CLI binary (~30 options)
├── gui.rs              # GUI binary (optional, feature "gui")
├── i18n.rs             # Internationalization (YAML locale loading)
├── exiftool.rs         # ExifTool struct: read API + write API + dispatch
├── error.rs            # Error types
├── value.rs            # Value enum (String, URational, Binary, etc.)
├── tag.rs              # Tag, TagGroup, TagId structs
├── file_type.rs        # 115 file types, magic detection
├── composite.rs        # 16 computed tags (GPS, ImageSize, etc.)
├── geolocation.rs      # Reverse geocoding from Geolocation.dat
├── config.rs           # .ExifTool_config parser
├── formats/            # 29 format readers (one per file)
├── metadata/           # Cross-format: exif.rs, iptc.rs, xmp.rs, makernotes.rs
├── tags/               # Tag tables + print conversions
│   ├── exif.rs         # Hand-written EXIF tags + print conv
│   ├── iptc.rs         # IPTC tags
│   ├── makernotes.rs   # MakerNotes tag lookup (9 manufacturers)
│   ├── canon_sub.rs    # Canon CameraSettings/ShotInfo decoders
│   ├── nikon_conv.rs   # Nikon print conversions
│   ├── sony_conv.rs    # Sony print conversions
│   ├── generated.rs    # AUTO-GENERATED: 4,286 tag names
│   └── print_conv_generated.rs  # AUTO-GENERATED: 17,592 print conversions
├── writer/             # 15 writer modules
│   ├── jpeg_writer.rs  # JPEG segment rewriting
│   ├── exif_writer.rs  # TIFF/EXIF IFD building
│   ├── xmp_writer.rs   # XMP XML generation
│   ├── iptc_writer.rs  # IPTC-IIM encoding
│   └── ...             # png, tiff, webp, mp4, psd, pdf, matroska, etc.
└── scripts/
    ├── gen_tags.pl      # Generate tag names from Perl source
    └── gen_print_conv.pl # Generate print conversions from Perl source
```

## Key Commands

```bash
cargo build --release                    # Build CLI only
cargo build --release --features gui     # Build CLI + GUI
cargo test                               # Run tests (22 unit + 3 doc)
cargo run -- -s photo.jpg                # Run CLI
cargo run --features gui --bin exiftool-rs-gui  # Run GUI
cargo run --features gui --bin exiftool-rs-gui -- -lang fr  # GUI in French

# Regenerate from Perl source:
perl scripts/gen_tags.pl ../exiftool/lib > src/tags/generated.rs
perl scripts/gen_print_conv.pl ../exiftool/lib > src/tags/print_conv_generated.rs
```

## Key Design Decisions

1. **No external dependencies for formats** — all parsers are hand-written
2. **Auto-generated tag tables** — Perl scripts extract from ExifTool source
3. **Fallback chain for tag lookup** — hand-written tables → generated tables
4. **Print conversion chain** — hand-written → manufacturer-specific → generated
5. **JPEG merge mode** — reads existing EXIF, applies changes, rebuilds
6. **Geolocation.dat** — uses ExifTool's binary database directly (no conversion)
7. **GUI is optional** — behind `gui` feature flag, no GUI deps in default build
8. **i18n via YAML** — locale files in `locales/`, 23 languages, UI strings prefixed `_ui.`

## GUI Architecture

- `src/gui.rs` — standalone binary using `eframe`/`egui`
- `src/i18n.rs` — loads YAML locale files, provides `ui_text()` and `tag_description()`
- `locales/*.yml` — 23 language files (3230 tag descriptions + 19 UI strings each)
- `assets/icon.png` — application icon (also embedded in Windows `.exe` via `build.rs`)
- `build.rs` — Windows-only: embeds `assets/icon.ico` into the executable via `winres`

## Important Files

- `generated.rs` and `print_conv_generated.rs` are AUTO-GENERATED — don't edit
- `exiftool.rs` contains both read and write logic — it's the largest file
- `file_type.rs` has the FileType enum with 115 variants
- `metadata/exif.rs` is the core EXIF IFD parser, also handles MakerNote dispatch

## Testing

```bash
# Run against ExifTool's full test suite:
for f in ../exiftool/t/images/*; do target/release/exiftool -s "$f"; done

# Compare with Perl:
diff <(target/release/exiftool -s -n photo.jpg) <(perl ../exiftool/exiftool -s -n photo.jpg)
```

## Adding a New Format

1. Create `src/formats/myformat.rs` with `pub fn read_myformat(data: &[u8]) -> Result<Vec<Tag>>`
2. Add `pub mod myformat;` to `src/formats/mod.rs`
3. Add `FileType::MyFormat` variant to `src/file_type.rs` (enum + description + mime + extensions + magic)
4. Add dispatch in `ExifTool::process_file()` in `src/exiftool.rs`

## Adding Write Support

1. Create `src/writer/myformat_writer.rs`
2. Add to `src/writer/mod.rs`
3. Add dispatch in `ExifTool::apply_changes()` in `src/exiftool.rs`
4. Add `fn write_myformat()` method to `ExifTool`
