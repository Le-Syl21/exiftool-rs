#!/bin/sh
# Write expected.txt from Perl ExifTool, in print mode and with -n:
#   fixture <TAB> print|numeric <TAB> Group1:Tag <TAB> value
# The System group and ExifToolVersion depend on the machine, not the file.
# Usage: make_expected.sh <path to the exiftool script> > expected.txt
set -e
et="$1"
cd "$(dirname "$0")"
for f in samsung.srw serial.srw percent75.xmp percent3.xmp trailing-slash.xmp; do
    for mode in print numeric; do
        if [ "$mode" = numeric ]; then n=-n; else n=; fi
        perl "$et" -s -G1 $n "$f" | perl -ne '
            my ($g, $t, $v) = /^\[(.*?)\]\s+(\S+)\s+: (.*)$/ or next;
            next if $g eq "System" or $t eq "ExifToolVersion";
            print join("\t", "'"$f"'", "'"$mode"'", "$g:$t", $v), "\n";
        '
    done
done
