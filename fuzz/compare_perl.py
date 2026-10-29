#!/usr/bin/env python3
"""Read files with the port's exiftool-rs and with Perl's exiftool, both
under a 4 GB address-space limit and a time limit, and compare how each
ends: exit status, time, peak memory, and the output (diffed).

usage: compare_perl.py EXIFTOOL_DIR FILE... [-- EXIFTOOL_ARGS]
(default arguments: -a -u -G1)
"""
import difflib
import os
import resource
import subprocess
import sys
import time

here = os.path.dirname(os.path.abspath(__file__))
port = os.path.join(here, "..", "target", "release", "exiftool-rs")
argv = sys.argv[1:]
args = ["-a", "-u", "-G1"]
if "--" in argv:
    k = argv.index("--")
    argv, args = argv[:k], argv[k + 1:]
exiftool, files = argv[0], argv[1:]


def run(cmd):
    t = time.time()
    p = subprocess.Popen(["prlimit", "--as=4000000000", "--", "timeout", "120"] + cmd, stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
    out = p.stdout.read()
    _, status, ru = os.wait4(p.pid, 0)
    return os.waitstatus_to_exitcode(status), time.time() - t, ru.ru_maxrss // 1024, out.decode("utf-8", "replace")


for f in files:
    a = run([port] + args + [f])
    b = run(["perl", os.path.join(exiftool, "exiftool")] + args + [f])
    print(f"== {f}")
    print(f"   port: rc={a[0]} {a[1]:.1f}s {a[2]} MB   perl: rc={b[0]} {b[1]:.1f}s {b[2]} MB")
    diff = list(difflib.unified_diff(b[3].splitlines(), a[3].splitlines(), "perl", "port", lineterm="", n=0))
    for line in diff[:30]:
        print("   " + line[:200])
    if not diff:
        print("   (same output)")
