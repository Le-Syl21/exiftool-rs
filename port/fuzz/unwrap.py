#!/usr/bin/env python3
"""Turn `extract` or `write` artifacts back into the files the targets read:
the first byte picks the extension (extensions.txt) and the options, the
rest is the file.

usage: unwrap.py OUT_DIR ARTIFACT...

Writes OUT_DIR/<artifact name>.<ext> and prints its extension and the
options mode (extract: 0 default, 1 print_conv off; write: the group
deleted, 0..3).
"""
import os
import sys

here = os.path.dirname(os.path.abspath(__file__))
exts = open(os.path.join(here, "extensions.txt")).read().split()
out = sys.argv[1]
os.makedirs(out, exist_ok=True)
for a in sys.argv[2:]:
    data = open(a, "rb").read()
    if not data:
        continue
    ext = exts[data[0] % len(exts)]
    mode = data[0] // len(exts)
    dst = os.path.join(out, os.path.basename(a) + "." + ext)
    open(dst, "wb").write(data[1:])
    print(f"{dst} ext={ext} mode={mode} size={len(data) - 1}")
