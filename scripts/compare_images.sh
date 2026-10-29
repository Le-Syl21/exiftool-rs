#!/bin/sh
# Compare everything extracted from each test image, perl against the port
# (scripts/dump_info.pl under both). Prints the files that differ.
#   sh scripts/compare_images.sh [EXIFTOOL_DIR] [GLOB]
here=$(cd "$(dirname "$0")/.." && pwd)
src=$(cd "${1:-${EXIFTOOL_DIR:-$here/../exiftool}}" && pwd)
glob=${2:-*}
work="$here/target/images"
mkdir -p "$work/perl" "$work/port"
cargo build --release --example run_t --manifest-path "$here/Cargo.toml" -j2 >/dev/null 2>&1
bin="$here/target/release/examples/run_t"
cd "$src"
# the optional modules the host's Perl lacks, which the port then does
# without (FileCreateDate on Linux, from File::StatX, on every file;
# scripts/cli_cases_full_modules.txt compares them against a Perl that has
# them)
without="File::StatX POSIX::strptime IO::String"
same=0; diff=0; bad=""
for f in t/images/$glob; do
    [ -f "$f" ] || continue
    n=$(basename "$f")
    prlimit --as=4000000000 -- timeout 300 perl -I"$src/lib" "$here/scripts/dump_info.pl" "$f" > "$work/perl/$n.txt" 2>&1
    env -u EXIFTOOL_LIB EXIFTOOL_WITHOUT="$without" prlimit --as=4000000000 -- timeout 300 "$bin" "$here/scripts/dump_info.pl" "$f" > "$work/port/$n.txt" 2>&1
    # FileAccessDate is the access time of the file, which the first of the
    # two reads may update (relatime does once a day): not a port difference.
    sed -i '/FileAccessDate = /d' "$work/perl/$n.txt" "$work/port/$n.txt"
    if cmp -s "$work/perl/$n.txt" "$work/port/$n.txt"; then same=$((same+1)); else diff=$((diff+1)); bad="$bad $n"; fi
done
echo "$same identical, $diff different"
[ -n "$bad" ] && echo "different:$bad"
exit 0
