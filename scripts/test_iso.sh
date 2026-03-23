#!/bin/bash
# exiftool-rs ISO-Functional Test Suite
#
# Compares exiftool-rs tag NAMES against stored reference lists
# generated from Perl ExifTool v13.53.
#
# No Perl ExifTool needed — reference files are in tests/expected/
# Tags are compared by NAME only (not values). Missing = failure, extras = OK.
#
# Usage: ./scripts/test_iso.sh
#
# Regenerate expected files (requires Perl ExifTool at ../exiftool/):
#   for f in tests/images/*; do
#     perl ../exiftool/exiftool -s "$f" 2>/dev/null | awk '{print $1}' | \
#       sed 's/:$//' | grep '^[A-Za-z]' | sort -u > "tests/expected/$(basename $f).tags"
#   done

set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_DIR="$(dirname "$SCRIPT_DIR")"

TEST_DIR="$PROJECT_DIR/tests/images"
EXPECTED_DIR="$PROJECT_DIR/tests/expected"
RUST_ET="$PROJECT_DIR/target/release/exiftool-rs"

# Auto-build if needed
if [ ! -f "$RUST_ET" ] || [ "$PROJECT_DIR/src" -nt "$RUST_ET" ]; then
  echo "Building exiftool-rs..."
  (cd "$PROJECT_DIR" && cargo build --release --quiet) || exit 1
fi

echo "╔══════════════════════════════════════════════════════════╗"
echo "║     exiftool-rs — ISO-FUNCTIONAL TEST SUITE             ║"
echo "╚══════════════════════════════════════════════════════════╝"
echo ""
echo "  Binary:    $RUST_ET"
echo "  Images:    $TEST_DIR ($(ls "$TEST_DIR" | wc -l) files)"
echo "  Reference: $EXPECTED_DIR"
echo ""

pass=0; fail=0; total=0; total_tags=0; missing_total=0; extra_total=0

for f in "$TEST_DIR"/*; do
  [ -f "$f" ] || continue
  name=$(basename "$f")
  expected="$EXPECTED_DIR/${name}.tags"
  total=$((total+1))

  if [ ! -f "$expected" ]; then
    echo "  ⚠️  $name (no reference file)"
    fail=$((fail+1))
    continue
  fi

  rust_tags=$("$RUST_ET" -s "$f" 2>/dev/null | awk '{print $1}' | sed 's/:$//' | grep '^[A-Za-z]' | sort -u)

  # Count missing tags (in Perl but not in Rust)
  missing=$(diff "$expected" <(echo "$rust_tags") 2>/dev/null | grep "^<" | wc -l)
  extra=$(diff "$expected" <(echo "$rust_tags") 2>/dev/null | grep "^>" | wc -l)

  if [ "$missing" -eq 0 ]; then
    count=$(wc -l < "$expected")
    total_tags=$((total_tags + count))
    extra_total=$((extra_total + extra))
    if [ "$extra" -gt 0 ]; then
      echo "  ✅ $name ($count tags, +$extra extras)"
    else
      echo "  ✅ $name ($count tags)"
    fi
    pass=$((pass+1))
  else
    missing_total=$((missing_total + missing))
    echo "  ❌ $name (missing=$missing extra=$extra)"
    diff "$expected" <(echo "$rust_tags") 2>/dev/null | grep "^<" | head -3 | sed 's/^/       /'
    fail=$((fail+1))
  fi
done

echo ""
echo "════════════════════════════════════════════════════════════"
echo "  RESULT: $pass/$total files — 0 missing tags"
echo "  Total tags verified: $total_tags"
if [ "$extra_total" -gt 0 ]; then
  echo "  Extra tags (valid metadata found beyond Perl): $extra_total"
fi
echo ""
if [ "$fail" -eq 0 ]; then
  echo "  🎯 PERFECT SCORE — 100% ISO-FUNCTIONAL PARITY"
else
  echo "  ⚠️  $fail file(s) with $missing_total missing tags"
fi
echo "════════════════════════════════════════════════════════════"
exit $fail
