"""Record what ExifTool reports for the Canon fixtures of issues #29, #30, #35.

Usage, from the repository root:
    python3 tests/issues/canon/make_expected.py perl /path/to/exiftool \
        > tests/issues/canon/expected.txt

The arguments are the command that runs ExifTool (a `docker run ... perl
/et/exiftool` works too, with the repository mounted at the same path). Each
line, tab-separated: fixture, `print` / `numeric` (without / with -n), the tag
as `Group1:Name`, its value. Every tag is kept but the System and ExifTool
groups, which describe the file on disk and the ExifTool version. Values are
the text of ExifTool's JSON, so a number keeps the digits ExifTool printed.
"""
import json
import subprocess
import sys

FIXTURES = [
    "tests/issues/canon/focal.crw",  # 29
    "tests/issues/canon/iso.crw",  # 30
    "tests/issues/canon/lens.tif",  # 35
]


def main():
    exiftool = sys.argv[1:]
    for path in FIXTURES:
        for mode, flags in (("print", []), ("numeric", ["-n"])):
            args = exiftool + ["-j", "-a", "-G1"] + flags + [path]
            out = subprocess.run(args, capture_output=True, text=True, check=True).stdout
            tags = json.loads(out, parse_float=str, parse_int=str)[0]
            for key, value in tags.items():
                if key == "SourceFile" or key.split(":")[0] in ("System", "ExifTool"):
                    continue
                print("\t".join([path.rsplit("/", 1)[1], mode, key, str(value)]))


main()
