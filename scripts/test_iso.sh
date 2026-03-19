#!/bin/bash
# ISO-Functional Test: compare exiftool-rs vs Perl ExifTool
#
# Usage: ./scripts/test_iso.sh [perl_exiftool_path] [test_images_dir]
#
# Defaults:
#   perl exiftool: searches PATH, then ../exiftool/exiftool
#   test images:   tests/images/ (bundled in repo)

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_DIR="$(dirname "$SCRIPT_DIR")"

# Find Perl ExifTool
if [ -n "$1" ]; then
  PERL_ET="$1"
elif command -v exiftool &>/dev/null; then
  PERL_ET="exiftool"
elif [ -f "$PROJECT_DIR/../exiftool/exiftool" ]; then
  PERL_ET="perl $PROJECT_DIR/../exiftool/exiftool"
else
  echo "Error: Perl ExifTool not found."
  echo "Install it (apt install libimage-exiftool-perl) or pass path as argument."
  exit 2
fi

# Test images directory
TEST_DIR="${2:-$PROJECT_DIR/tests/images}"
if [ ! -d "$TEST_DIR" ] || [ -z "$(ls "$TEST_DIR"/*.jpg 2>/dev/null)" ]; then
  echo "Error: No JPEG test images found in $TEST_DIR"
  exit 2
fi

# Rust binary
RUST_ET="$PROJECT_DIR/target/release/exiftool-rs"
if [ ! -f "$RUST_ET" ]; then
  echo "Building exiftool-rs..."
  (cd "$PROJECT_DIR" && cargo build --release) || exit 1
fi

echo "╔══════════════════════════════════════════════════════════╗"
echo "║     exiftool-rs — ISO-FUNCTIONAL TEST SUITE             ║"
echo "╚══════════════════════════════════════════════════════════╝"
echo ""
echo "  Rust:   $RUST_ET"
echo "  Perl:   $PERL_ET"
echo "  Images: $TEST_DIR"
echo ""

pass=0; fail=0; total=0; total_tags=0
for f in "$TEST_DIR"/*.jpg; do
  [ -f "$f" ] || continue
  name=$(basename "$f" .jpg)
  total=$((total+1))

  perl_tags=$($PERL_ET -s "$f" 2>/dev/null | awk '{print $1}' | grep '^[A-Za-z]' | sort -u)
  rust_tags=$("$RUST_ET" -s "$f" 2>/dev/null | awk '{print $1}' | grep '^[A-Za-z]' | sort -u)

  d=$(diff <(echo "$perl_tags") <(echo "$rust_tags"))

  if [ -z "$d" ]; then
    count=$(echo "$perl_tags" | wc -l)
    total_tags=$((total_tags + count))
    echo "  ✅ $name ($count tags)"
    pass=$((pass+1))
  else
    missing=$(echo "$d" | grep "^<" | wc -l)
    extra=$(echo "$d" | grep "^>" | wc -l)
    echo "  ❌ $name (missing=$missing extra=$extra)"
    echo "$d" | head -10 | sed 's/^/       /'
    fail=$((fail+1))
  fi
done

echo ""
echo "════════════════════════════════════════════════════════════"
echo "  RESULT: $pass/$total files — $total_tags total tags verified"
echo ""
if [ "$fail" -eq 0 ]; then
  echo "  🎯 PERFECT SCORE — 100% ISO-FUNCTIONAL PARITY"
else
  echo "  ⚠️  $fail file(s) with differences"
fi
echo "════════════════════════════════════════════════════════════"
exit $fail
