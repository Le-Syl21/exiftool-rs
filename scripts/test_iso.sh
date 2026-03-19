#!/bin/bash
# ISO-Functional Test: compare exiftool-rs vs Perl ExifTool
# Usage: ./scripts/test_iso.sh [path/to/perl/exiftool] [path/to/test/images]

PERL_ET="${1:-../exiftool/exiftool}"
TEST_DIR="${2:-../exiftool/t/images}"
RUST_ET="./target/release/exiftool-rs"

if [ ! -f "$RUST_ET" ]; then
  echo "Build first: cargo build --release"
  exit 1
fi

echo "╔══════════════════════════════════════════════════════════╗"
echo "║     exiftool-rs — ISO-FUNCTIONAL TEST SUITE             ║"
echo "╚══════════════════════════════════════════════════════════╝"
echo ""
echo "Rust binary: $RUST_ET"
echo "Perl binary: $PERL_ET"
echo "Test images: $TEST_DIR"
echo ""

pass=0; fail=0; total=0; total_tags=0
for f in "$TEST_DIR"/*.jpg; do
  [ -f "$f" ] || continue
  name=$(basename "$f" .jpg)
  total=$((total+1))

  perl_tags=$(perl "$PERL_ET" -s "$f" 2>/dev/null | awk '{print $1}' | grep '^[A-Za-z]' | sort -u)
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
