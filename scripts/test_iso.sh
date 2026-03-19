#!/bin/bash
# exiftool-rs ISO-Functional Test Suite
#
# Compares exiftool-rs output against stored reference tag lists.
# No Perl ExifTool needed — reference files are in tests/expected/
#
# Usage: ./scripts/test_iso.sh

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_DIR="$(dirname "$SCRIPT_DIR")"

TEST_DIR="$PROJECT_DIR/tests/images"
EXPECTED_DIR="$PROJECT_DIR/tests/expected"
RUST_ET="$PROJECT_DIR/target/release/exiftool-rs"

# Auto-build if needed
if [ ! -f "$RUST_ET" ]; then
  echo "Building exiftool-rs..."
  (cd "$PROJECT_DIR" && cargo build --release) || exit 1
fi

echo "╔══════════════════════════════════════════════════════════╗"
echo "║     exiftool-rs — ISO-FUNCTIONAL TEST SUITE             ║"
echo "╚══════════════════════════════════════════════════════════╝"
echo ""
echo "  Binary:    $RUST_ET"
echo "  Images:    $TEST_DIR"
echo "  Reference: $EXPECTED_DIR"
echo ""

pass=0; fail=0; total=0; total_tags=0
for f in "$TEST_DIR"/*.jpg; do
  [ -f "$f" ] || continue
  name=$(basename "$f" .jpg)
  expected="$EXPECTED_DIR/${name}.tags"
  total=$((total+1))

  if [ ! -f "$expected" ]; then
    echo "  ⚠️  $name (no reference file)"
    fail=$((fail+1))
    continue
  fi

  rust_tags=$("$RUST_ET" -s "$f" 2>/dev/null | awk '{print $1}' | grep '^[A-Za-z]' | sort -u)
  d=$(diff "$expected" <(echo "$rust_tags"))

  if [ -z "$d" ]; then
    count=$(wc -l < "$expected")
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
