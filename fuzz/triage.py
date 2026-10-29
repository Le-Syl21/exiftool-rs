#!/usr/bin/env python3
"""Replay each artifact of a fuzz target and group them by cause.

usage: triage.py TARGET [ARTIFACT_DIR]

A panic is grouped by its `panicked at FILE:LINE` (the first location in
the port's sources); a crash, a stack overflow, a timeout or an OOM by the
first frames of the port's code in its stack, symbolized with addr2line
(llvm-symbolizer is not in the nightly's llvm-tools).
"""
import collections
import os
import re
import subprocess
import sys

here = os.path.dirname(os.path.abspath(__file__))
target = sys.argv[1]
arts = sys.argv[2] if len(sys.argv) > 2 else os.path.join(here, "artifacts", target)
binary = os.path.join(here, "target", "x86_64-unknown-linux-gnu", "release", target)

frame_re = re.compile(r"\(" + re.escape(binary) + r"\+(0x[0-9a-f]+)\)")


def symbolize(addrs):
    if not addrs:
        return []
    out = subprocess.run(["addr2line", "-f", "-C", "-i", "-e", binary] + addrs, capture_output=True, text=True).stdout.splitlines()
    return [(out[i], out[i + 1]) for i in range(0, len(out) - 1, 2)]


def cause(path):
    try:
        p = subprocess.run([binary, "-timeout=20", "-rss_limit_mb=2048", "-runs=1", path], capture_output=True, text=True, errors="replace", timeout=120)
        err = p.stderr
    except subprocess.TimeoutExpired:
        return "hang (no end in 120 s)", ""
    m = re.search(r"panicked at (\S+:\d+):\d+:\n(.*)", err)
    if m:
        return "panic " + m.group(1), m.group(2)[:200]
    if p.returncode == 0:
        return "no reproduction", ""
    # (the kind from the error line only: an artifact's own name, printed
    # too, may be "timeout-...")
    m = re.search(r"ERROR: (?:libFuzzer|AddressSanitizer|LeakSanitizer): ([\w-]+(?: [\w-]+)?)", err)
    kind = m.group(1) if m else ("stack overflow" if "has overflowed its stack" in err else "?")
    addrs = frame_re.findall(err)
    frames = []
    for fn, loc in symbolize(addrs[:200]):
        if "/port/src/" in loc:
            frames.append(loc.split("/port/")[-1].split(" ")[0] + " " + fn.split("(")[0][-60:])
        if len(frames) >= 3:
            break
    m = re.search(r"Executed .* in (\d+) ms", err)
    return kind + " " + (frames[0] if frames else "(no port frame)"), "; ".join(frames[1:]) + (f" [{m.group(1)} ms]" if m else "")


groups = collections.OrderedDict()
for name in sorted(os.listdir(arts)):
    path = os.path.join(arts, name)
    key, detail = cause(path)
    groups.setdefault(key, []).append((name, os.path.getsize(path), detail))

for key, items in groups.items():
    items.sort(key=lambda i: i[1])
    print(f"{len(items):4}  {key}")
    print(f"      smallest {items[0][0]} ({items[0][1]} bytes) {items[0][2]}")
