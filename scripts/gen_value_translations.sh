#!/usr/bin/env bash
# Regenerate the PrintConv value-translation tables from ExifTool 13.59.
#
# Two artifacts per language, both derived from the Perl source / real output:
#   locales/values/<lang>.tsv       flat  tag\tenglish\ttranslation  (Lang PrintConv)
#   locales/values/<lang>.over.tsv  group-scoped corrections from real corpus output
#
# Point EXIFTOOL_LIB / the exiftool checkout at your ExifTool 13.59 tree.
set -euo pipefail
cd "$(dirname "$0")/.."
export LC_ALL=C

mkdir -p locales/values

# exiftool-rs code -> ExifTool Lang code
declare -A MAP=(
  [fr]=fr [de]=de [es]=es [it]=it [ja]=ja [ko]=ko [nl]=nl [pl]=pl [ru]=ru
  [cs]=cs [fi]=fi [sk]=sk [sv]=sv [tr]=tr [zh]=zh_cn [zh_tw]=zh_tw
  [en_ca]=en_ca [en_gb]=en_gb
)

echo "1/2 flat tables (Lang PrintConv) ..."
for code in "${!MAP[@]}"; do
  perl scripts/lang_values.pl "${MAP[$code]}" | sort -u > "locales/values/$code.tsv"
done

echo "2/2 group-scoped overrides (real corpus output) ..."
python3 scripts/gen_value_overrides.py

echo "done."
