#!/usr/bin/env python3
"""Compare Perl's exiftool and the port's exiftool-rs on a test corpus
(tests/corpus/README.md; fetched with scripts/fetch_corpus.sh), file by
file, in two modes: printed values and raw values, every tag (-a -u -G1).
Prints the files that differ, then the totals and the timings.

    python3 scripts/compare_corpus.py [CORPUS] [EXIFTOOL_DIR]

CORPUS defaults to rawsamples; EXIFTOOL_DIR to ../exiftool (next to the repository). Outputs go to
target/corpus_cmp/<corpus>/{perl,port}/<file>.<mode>.json.
"""
import json
import os
import subprocess
import sys
import time

here = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
name = sys.argv[1] if len(sys.argv) > 1 else "rawsamples"
src = os.path.abspath(sys.argv[2] if len(sys.argv) > 2 else os.path.join(here, "..", "exiftool"))
corpus = os.path.join(here, "target", "corpus", name)
work = os.path.join(here, "target", "corpus_cmp", name)
port = os.path.join(here, "target", "release", "exiftool-rs")
limit = ["prlimit", "--as=4000000000", "--", "timeout", "3600"]
# tags that differ between two reads of the same file
VOLATILE = {"FileAccessDate", "FileInodeChangeDate", "FileModifyDate", "FileInodeNumber"}
MODES = {"print": ["-j", "-a", "-u", "-G1"], "num": ["-j", "-a", "-u", "-G1", "-n"]}


def run(cmd):
    """Run on the whole corpus at once (one process per side and mode, as a
    user would), return {file: tags} and the time taken."""
    t = time.monotonic()
    p = subprocess.run(limit + cmd + ["-r", "."], cwd=corpus, capture_output=True, stdin=subprocess.DEVNULL)
    secs = time.monotonic() - t
    try:
        data = json.loads(p.stdout.decode("utf-8", "replace") or "[]")
    except json.JSONDecodeError as e:
        data = []
        print(f"bad JSON from {cmd[0]}: {e}")
    out = {}
    for d in data:
        f = d.get("SourceFile")
        out[f] = {k: v for k, v in d.items() if k.split(":")[-1] not in VOLATILE}
    return out, secs, p.stderr.decode("utf-8", "replace")


def main():
    if not os.path.isdir(corpus):
        sys.exit(f"no corpus at {corpus}: run scripts/fetch_corpus.sh {name}")
    subprocess.run(["cargo", "build", "--release", "--bin", "exiftool-rs", "-j8"], cwd=here, capture_output=True)
    env_note = {}
    files = set()
    bad = {}
    times = {"perl": 0.0, "port": 0.0}
    for mode, args in MODES.items():
        res = {}
        for side, cmd in (("perl", ["perl", os.path.join(src, "exiftool")]), ("port", [port])):
            res[side], secs, err = run(cmd + args)
            times[side] += secs
            print(f"{mode} {side}: {secs:.1f} s, {len(res[side])} files", flush=True)
            d = os.path.join(work, side)
            os.makedirs(d, exist_ok=True)
            with open(os.path.join(d, f"all.{mode}.json"), "w") as f:
                json.dump(res[side], f, indent=1, sort_keys=True, ensure_ascii=False)
            with open(os.path.join(d, f"stderr.{mode}.txt"), "w") as f:
                f.write(err)
        for f in set(res["perl"]) | set(res["port"]):
            files.add(f)
            a, b = res["perl"].get(f), res["port"].get(f)
            if a != b:
                tags = sorted(k for k in set(a or {}) | set(b or {}) if (a or {}).get(k) != (b or {}).get(k))
                bad.setdefault(f, set()).update(f"{mode}:{t}" for t in tags)
    same = len(files) - len(bad)
    print(f"{same} identical, {len(bad)} different (of {len(files)} files, both modes)")
    for f in sorted(bad):
        tags = sorted(bad[f])
        print(f"different {f}: {', '.join(tags[:6])}{' ...' if len(tags) > 6 else ''}")
    gain = (times["perl"] - times["port"]) / times["perl"] * 100 if times["perl"] else 0
    print(f"time: perl {times['perl']:.1f} s, port {times['port']:.1f} s ({gain:+.0f} %)")


main()
