"""Record what ExifTool reports for the GPS tags of each fixture.

Usage, from the repository root:
    python3 tests/gps/make_expected.py /path/to/exiftool > tests/gps/expected.txt

Each line, tab-separated: fixture, `print` / `numeric` (without / with -n),
the tag as `Group1:Name`, its value. Only the GPS group and the GPS composites
are kept. Values are the text of ExifTool's JSON, so a number keeps the digits
ExifTool printed.
"""
import json
import subprocess
import sys

FIXTURES = [
    "tests/gps/gps_tokyo.jpg",
    "tests/gps/gps_tokyo.cr3",
    "tests/gps/gps_southwest.jpg",
    "tests/gps/gps_southwest.cr3",
    # ExifTool's own file, from the parity corpus
    "tests/images/GPS.jpg",
]


def main():
    exiftool = sys.argv[1]
    for path in FIXTURES:
        for mode, flags in (("print", []), ("numeric", ["-n"])):
            args = ["perl", exiftool, "-j", "-a", "-G1"] + flags + [path]
            out = subprocess.run(args, capture_output=True, text=True, check=True).stdout
            tags = json.loads(out, parse_float=str, parse_int=str)[0]
            for key, value in tags.items():
                if key.startswith("GPS:") or key.startswith("Composite:GPS"):
                    print("\t".join([path.rsplit("/", 1)[1], mode, key, str(value)]))


main()
