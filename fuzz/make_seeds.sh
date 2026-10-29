#!/bin/sh
# Build the starting corpora.
#
# extract and write: each file of ExifTool's t/images, of the repository's
# tests/images and tests/malformed, and the first 64 KiB of a sample of RAW
# files (RAW_DIR, one in eight), prefixed with the byte that makes the
# targets pick its own extension back.
# interp: a few programs' operands (patterns, templates, formats...).
#
# usage: make_seeds.sh EXIFTOOL_DIR [RAW_DIR]
set -eu
cd "$(dirname "$0")"
exiftool=${1:?usage: make_seeds.sh EXIFTOOL_DIR [RAW_DIR]}
raw=${2:-}
mkdir -p corpus/extract corpus/write corpus/interp

seed() { # FILE NAME [BYTES]
    ext=$(printf '%s' "${1##*.}" | tr 'A-Z' 'a-z')
    n=$(grep -nx "$ext" extensions.txt | cut -d: -f1) || return 0
    for t in extract write; do
        printf "\\$(printf '%03o' $((n - 1)))" > "corpus/$t/$2"
        if [ -n "${3:-}" ]; then head -c "$3" "$1" >> "corpus/$t/$2"; else cat "$1" >> "corpus/$t/$2"; fi
    done
}

for f in "$exiftool"/t/images/*.*; do
    if [ -f "$f" ]; then seed "$f" "pl-$(basename "$f")"; fi
done
for f in ../tests/images/*.*; do
    if [ -f "$f" ]; then seed "$f" "rs-$(basename "$f")"; fi
done
for f in ../tests/malformed/*.*; do
    if [ -f "$f" ]; then seed "$f" "malformed-$(basename "$f")"; fi
done
if [ -n "$raw" ]; then
    i=0
    find "$raw" -type f | sort | while read -r f; do
        i=$((i + 1))
        if [ $((i % 8)) -eq 0 ]; then seed "$f" "raw-$(basename "$f")" 65536; fi
    done
fi

# interp: program byte, then operands separated by 0xFF
op() { # NAME PROGRAM A B C
    { printf "\\$(printf '%03o' "$2")"; printf '%s\377%s\377%s' "$3" "$4" "$5"; } > "corpus/interp/$1"
}
op re1 0 '(\d+):(\d+)' '2024:01:02 12:34' ''
op re2 0 '^(?:(\w+)\s*=\s*"([^"]*)"\s*)+$' 'a = "b" c="d"' ''
op re3 1 '(?<y>\d{4})(?=-)' "caf\303\251 2024-10" ''
op re4 1 '\x{263a}|\p{L}+' "\342\230\272 abc" ''
op pk1 2 'n2 N/a* Z*' "\000\001\000\002\000\000\000\003abcdef\000" '1,2,abc,def'
op pk2 2 'C/a v2 H* (A2)3 x2 @4 w' "\003abc\001\000\002\000\377" '3,abc,1,2,ff'
op pk3 2 'f< d> q Q> j U0U*' "\000\000\200\077\100\011\041\373\124\104\055\030" '1.5,2.5,3,4'
op sp1 3 '%s %d %.3f %x %o %e %g %5.2s %-8s|%+d %%' 'abc,42,3.14159,255,8,1e10,0.0001,hello,x,-3' ''
op sp2 3 '%2$s %1$s %*d %-*.*f %vd %c %b %#x %#o' '1,2,3,4,5,6.5,1.22.333' ''
op tr1 4 'a-z' 'Hello World' 'A-Z'
op tr2 4 '\x00-\x1f\x7f' "a\001b\177c" ' '
op tr3 4 'a-zA-Z' 'counting letters' 'cd'
op ss1 5 'XYZ' 'abcdefghij' "\002\003"
op ss2 5 'q' "caf\303\251 cr\303\250me" "\375\002"
op u1 6 '' "caf\303\251 \342\230\272 \377\376" ''
op u2 6 "\000a\000b" "\376\377\000x\330\075\336\000" ''
op n1 7 '0x1f' '0b101' ''
op n2 7 'ff' '1e308' ''
op n3 7 '12abc' '-0.5e-3' ''

echo "$(ls corpus/extract | wc -l) seeds in corpus/extract and write, $(ls corpus/interp | wc -l) in corpus/interp"
