# Migration Guide

## From Perl ExifTool

exiftool-rs 1.0 is a faithful port of [ExifTool](https://exiftool.org/)
13.60: ExifTool's own modules, run by a Perl interpreter written in Rust.
Its output is ExifTool's, so a script that parses `exiftool`'s output works
on `exiftool-rs`'s unchanged. What differs:

- **The program's name** is `exiftool-rs`, never `exiftool`.
- **No Perl installation**: the optional Perl modules ExifTool uses
  (Compress::Raw::Lzma, Unicode::LineBreak, POSIX::strptime, File::StatX,
  Archive::Zip, the Brotli and Win32 modules...) are provided by the
  program, so a file needing one is read where a Perl lacking it would not.
- **`-ver -v`** lists what the program is built with (the Rust toolchain,
  its own Perl interpreter and libraries) instead of Perl's.
- Config files (`.ExifTool_config`), argfiles and `-if` expressions are
  still Perl, run by the port's interpreter as ExifTool runs them.

How closely it follows ExifTool, and how that is checked: docs/port/CLI.md
(the judges) and docs/port/ARCHITECTURE.md.

## From exiftool-rs 0.x

0.x was a reimplementation of ExifTool with its own readers and writers;
1.0 replaces it with the port. The Rust API is kept: the same crate
(`exiftool-rs`), the same path (`use exiftool_rs::...`), the same types and
signatures (`ExifTool`, `Options`, `Tag`, `Value`, `extract_info`,
`set_new_value`, `write_info`...), so code written against 0.14 builds
against 1.0. What it returns is now ExifTool's own: tag names, values,
groups, order and warnings as `exiftool` gives them, where 0.x had its own
formatting in places (`4 s` for an exposure time, `f/14.0` for an F-number).

The differences that matter to a program using the API, and the internal
modules 0.x exported that 1.0 does not (`formats`, `metadata`, `tags`,
`writer`, `encoding`, `geolocation`...), are listed in docs/port/API.md,
"Differences with the root crate".

The CLI accepts every option of ExifTool's `exiftool` application, where
0.x had a subset.
