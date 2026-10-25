#!/bin/sh
# Run ExifTool's whole test suite (t/*.t, unchanged) through the port.
#   sh scripts/run_suite.sh [EXIFTOOL_DIR]
# The scripts run in a copy of t/ under target/, so the ExifTool checkout
# is never written to. Prints one line per script and the totals.
set -e
here=$(cd "$(dirname "$0")/.." && pwd)
src=$(cd "${1:-$here/../../exiftool}" && pwd)
work="$here/target/suite"
mkdir -p "$work/out"
rsync -a "$src/t/" "$work/t/"
cargo build --release --example run_t --manifest-path "$here/Cargo.toml" -j2 >/dev/null 2>&1
bin="$here/target/release/examples/run_t"
cd "$work"
: > results.txt
for t in t/*.t; do
    n=$(basename "$t" .t)
    EXIFTOOL_LIB="$src/lib" timeout 300 "$bin" "$t" > "out/$n.txt" 2> "out/$n.err" && code=0 || code=$?
    plan=$(head -1 "out/$n.txt" | sed -n 's/^1\.\.//p')
    ok=$(grep -c '^ok ' "out/$n.txt" || true)
    nok=$(grep -c '^not ok' "out/$n.txt" || true)
    printf '%s\t%s\t%s\t%s\t%s\n' "$n" "${plan:-?}" "$ok" "$nok" "$code" >> results.txt
done
awk -F'\t' '{plan+=($2=="?"?0:$2); ok+=$3; if($2!="?" && $2==$3 && $4==0) full++; else bad=bad" "$1; n++}
    END{print n" scripts, "full" pass, "ok"/"plan" tests ok"; if (bad) print "failing:"bad}' results.txt
