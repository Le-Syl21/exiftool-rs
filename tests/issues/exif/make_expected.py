"""Record what ExifTool reports for each fixture of this folder.

Usage, from the repository root:
    python3 tests/issues/exif/make_expected.py /path/to/exiftool > tests/issues/exif/expected.txt

Each line, tab-separated: fixture, `print` / `numeric` (without / with -n),
the tag as `Group1:Name`, its value. Every tag is kept but those of the file
system and ExifToolVersion, which say nothing of the metadata. Values are the
text of ExifTool's JSON, so a number keeps the digits ExifTool printed.
"""
import json
import subprocess
import sys

FIXTURES = [
    "unicode.tif",
    "empty.tif",
    "plain.tif",
    "ascii.tif",
    "orientation0.tif",
    "orientation1.tif",
    "orientation6.tif",
    "orientation9.tif",
    "unknown_values.tif",
    "xp_unicode.tif",
]


def main():
    exiftool = sys.argv[1]
    for name in FIXTURES:
        for mode, flags in (("print", []), ("numeric", ["-n"])):
            args = ["perl", exiftool, "-j", "-a", "-G1"] + flags + ["tests/issues/exif/" + name]
            out = subprocess.run(args, capture_output=True, text=True, check=True).stdout
            tags = json.loads(out, parse_float=str, parse_int=str)[0]
            for key, value in tags.items():
                group = key.split(":", 1)[0]
                if key == "SourceFile" or group in ("System", "File") or key.endswith(":ExifToolVersion"):
                    continue
                print("\t".join([name, mode, key, str(value)]))


main()
