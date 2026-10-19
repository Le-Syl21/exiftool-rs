"""Record what ExifTool does with each case of cases.txt on each fixture.

Usage, from the repository root:
    python3 tests/issue22/make_expected.py /path/to/exiftool > tests/issue22/expected.txt

Each line, tab-separated: fixture, case, `changed` / `unchanged` / `error`
(ExifTool's "created" / "copied" / anything else), then the output's chunks as
TYPE[keyword]:CRC (the CRC covers type and data), and `trailer:N` for N bytes
after IEND.
"""
import os
import struct
import subprocess
import sys
import tempfile

HERE = "tests/issue22/"
FIXTURES = ["text", "itxt", "ztxt", "lang", "sink"]


def chunks(data):
    out, p = [], 8
    while p + 12 <= len(data):
        n, = struct.unpack(">I", data[p:p + 4])
        typ = data[p + 4:p + 8].decode("latin-1")
        body = data[p + 8:p + 8 + n]
        s = typ
        if typ in ("tEXt", "zTXt", "iTXt"):
            s += "[" + body.split(b"\0")[0].decode("latin-1") + "]"
        s += ":%08x" % struct.unpack(">I", data[p + 8 + n:p + 12 + n])[0]
        out.append(s)
        p += 12 + n
        if typ == "IEND":
            break
    if p < len(data):
        out.append("trailer:%d" % (len(data) - p))
    return out


def main():
    exiftool = sys.argv[1]
    cases = []
    for line in open(HERE + "cases.txt", encoding="utf-8"):
        if line.strip() and not line.startswith("#"):
            name, *ops = line.rstrip("\n").split("\t")
            cases.append((name, ops))
    with tempfile.TemporaryDirectory() as tmp:
        for fixture in FIXTURES:
            src = HERE + "issue22_" + fixture + ".png"
            for name, ops in cases:
                dst = os.path.join(tmp, fixture + "." + name + ".png")
                args = ["perl", exiftool] + ["-" + op for op in ops] + ["-o", dst, src]
                res = subprocess.run(args, capture_output=True, text=True)
                if "1 image files created" in res.stdout:
                    status = "changed"
                elif "1 image files copied" in res.stdout:
                    status = "unchanged"
                else:
                    status = "error"
                listing = chunks(open(dst, "rb").read()) if os.path.exists(dst) else []
                print("\t".join([fixture, name, status] + listing))


main()
