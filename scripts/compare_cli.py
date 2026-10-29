#!/usr/bin/env python3
"""Run each command line of cli_cases.txt with Perl's exiftool and with the
port, and compare what they print and, for the writing cases, the files they
leave. Prints the cases that differ, then the totals and the timings.

    python3 scripts/compare_cli.py [EXIFTOOL_DIR] [FILTER]

EXIFTOOL_DIR is ExifTool's source tree (default ../../exiftool); FILTER keeps
the cases whose line contains it; CLI_CASES=FILE reads the cases from FILE;
PORT_BIN=PATH runs the port's own exiftool-rs binary instead of run_t + script.
Outputs go to target/cli/<n>/{perl,port}.

The case kinds (KIND|ARGUMENTS) are R (read-only) and W (writes, the files
are compared too), and two more:
  S   the port alone, against its golden output scripts/cli_golden/<args>.txt
      with the version numbers masked: a case whose output is the port's own
      by design (-ver -v); CLI_GOLDEN_UPDATE=1 writes the golden files
  RM, WM  Perl needs an optional module that the host's Perl lacks and the
      port provides (Compress::Raw::Lzma, Unicode::LineBreak): Perl is then
      the container's (PERL_FULL_CMD, default scripts/full_modules/perl.sh)
PERL_CMD=CMD runs Perl as CMD for every case (scripts/full_modules/perl.sh
for the second judge set, cli_cases_full_modules.txt; see docs/port/CLI.md).
In the arguments, {SRC} is ExifTool's tree (the same files for both sides,
for what differs between two copies: FileCreateDate), and "< FILE" at the
end gives the command FILE (in its directory) on a pipe as its standard
input, which is otherwise /dev/null.
"""
import hashlib
import os
import re
import shlex
import shutil
import subprocess
import sys
import time

here = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
src = os.path.abspath(sys.argv[1] if len(sys.argv) > 1 else os.path.join(here, "..", "..", "exiftool"))
filt = sys.argv[2] if len(sys.argv) > 2 else None
work = os.path.join(here, "target", "cli")
port = os.path.join(here, "target", "release", "examples", "run_t")
script = os.path.join(src, "exiftool")
limit = ["prlimit", "--as=4000000000", "--", "timeout", "300"]
golden_dir = os.path.join(here, "scripts", "cli_golden")
full_modules = os.path.join(here, "scripts", "full_modules")


def command(text):
    """A command line from the environment, its program made absolute when
    it is a relative path (each case runs in a directory of its own)."""
    cmd = shlex.split(text)
    if cmd and "/" in cmd[0]:
        cmd[0] = os.path.abspath(cmd[0])
    return cmd


perl_cmd = command(os.environ["PERL_CMD"]) if os.environ.get("PERL_CMD") else ["perl"]
perl_full_cmd = command(os.environ.get("PERL_FULL_CMD") or os.path.join(full_modules, "perl.sh"))

# lines that differ between any two runs: the dates of the files
# themselves, random IDs, the inode, the clock
VOLATILE = re.compile(r"File (Access|Inode Change|Modification) Date/Time|File(Access|InodeChange|Modify)Date|"
                      r"New [GU]UID|New[GU]UID|File Inode Number|FileInodeNumber|"
                      r"^\[?\w*\]?\s*(Now|Age)\s*:|\"Now\"")
# the birth time of a file (FileCreateDate on Linux, with File::StatX), which
# differs between the two sides' copies (by more than the second masked as
# today's in -v3's raw numbers): kept only in the cases that read ExifTool's
# own files ({SRC}), the same on both sides
BIRTH = re.compile(r"File Creation Date/Time|FileCreateDate")
# the ProcessingTime tag: there, but its value a timing
PTIME = re.compile(r"(Processing ?Time\b[^0-9\n]*)[0-9][0-9.e+-]*")
TODAY = time.strftime("%Y:%m:%d")
# a date of today (a file's own dates in a format that does not name the
# tag: -s3, -csv, -l...)
NOW = re.compile(re.escape(TODAY) + r" \d\d:\d\d:\d\d(\.\d+)?([+-]\d\d:\d\d|Z)?")

# the optional modules the port provides that the host's Perl lacks, and
# which change what it does with no case asking for them (FileCreateDate on
# every file, the -d inverse conversions, ZIP from a pipe): the port does
# without them against the host's Perl (EXIFTOOL_WITHOUT), and has them
# against the container's (PERL_CMD, the RM/WM cases)
HOST_LACKS = "File::StatX POSIX::strptime IO::String"

CSV_IN = "SourceFile,Artist,XMP:Title\nimages/Canon.jpg,From CSV,CSV title\n"
JSON_IN = '[{"SourceFile":"images/Nikon.jpg","Artist":"From JSON","XMP:Subject":["a","b"]}]\n'


def cases():
    # (CLI_CASES names another case file, to try one's own cases)
    path = os.environ.get("CLI_CASES") or os.path.join(here, "scripts", "cli_cases.txt")
    with open(path) as f:
        for line in f:
            line = line.rstrip("\n")
            if not line or line.startswith("#"):
                continue
            kind, args = line.split("|", 1)
            if filt and filt not in line:
                continue
            yield kind, args


def prepare(dir):
    """A fresh copy of t/images (as images/) and the fixture directories."""
    shutil.rmtree(dir, ignore_errors=True)
    os.makedirs(dir)
    shutil.copytree(os.path.join(src, "t", "images"), os.path.join(dir, "images"), copy_function=shutil.copy2)
    # (the read-only cases name t/images: a copy too, so that a case that
    # writes after all cannot touch ExifTool's tree)
    shutil.copytree(os.path.join(dir, "images"), os.path.join(dir, "t", "images"), copy_function=shutil.copy2)
    for d in ("arg_files", "config_files", "fmt_files"):
        os.symlink(os.path.join(src, d), os.path.join(dir, d))
    # the 7z archives of the second judge set (copied: a symbolic link would
    # lead nowhere in the container of its reference Perl)
    shutil.copytree(os.path.join(full_modules, "7z"), os.path.join(dir, "7z"), copy_function=shutil.copy2)
    with open(os.path.join(dir, "csv_in.csv"), "w") as f:
        f.write(CSV_IN)
    with open(os.path.join(dir, "json_in.json"), "w") as f:
        f.write(JSON_IN)
    # an answer to a question on the standard input ("< yes.txt")
    with open(os.path.join(dir, "yes.txt"), "w") as f:
        f.write("y\n")


def files(dir):
    out = {}
    for root, dirs, names in os.walk(dir):
        dirs[:] = [d for d in dirs if not os.path.islink(os.path.join(root, d))]
        for n in names:
            p = os.path.join(root, n)
            if os.path.islink(p):
                continue
            with open(p, "rb") as f:
                out[os.path.relpath(p, dir)] = hashlib.sha256(f.read()).hexdigest()[:16]
    return out


def normalize(text, cwd, same_files=False):
    text = text.replace(cwd, "WORK").replace(src, "SRC")
    text = "".join(l for l in text.splitlines(True) if not VOLATILE.search(l) and (same_files or not BIRTH.search(l)))
    text = PTIME.sub(r"\1N", text)
    return NOW.sub("TODAY", text)


# what changes from one build of the port to another in its own -ver -v:
# the versions (but ExifTool's and perl's, which the port follows), and the
# target after the compiler's version
VERSIONS = re.compile(r"(?<![\w.])(?<!ExifTool )(?<!perl )\d+\.\d+(\.\d+)*")
TARGET = re.compile(r"^(Rust version [^(]*)\(.*\)$", re.M)


def masked(text):
    return VERSIONS.sub("N.N", TARGET.sub(r"\1(TARGET)", text))


def golden_check(args, rc, out, err):
    """An S case: the port's output, versions masked, against its golden file."""
    got = f"rc={rc}\n--- stdout\n{masked(out)}--- stderr\n{masked(err)}"
    path = os.path.join(golden_dir, re.sub(r"\W+", "_", args).strip("_") + ".txt")
    if os.environ.get("CLI_GOLDEN_UPDATE"):
        os.makedirs(golden_dir, exist_ok=True)
        with open(path, "w") as f:
            f.write(got)
    if not os.path.exists(path):
        return False
    with open(path) as f:
        return f.read() == got


def run(cmd, cwd, env, stdin=None):
    t = time.monotonic()
    if stdin:
        # (through a pipe, as the container of the second judge set gives
        # it to its Perl)
        with open(os.path.join(cwd, stdin), "rb") as f:
            p = subprocess.run(limit + cmd, cwd=cwd, env=env, input=f.read(), capture_output=True)
    else:
        p = subprocess.run(limit + cmd, cwd=cwd, env=env, stdin=subprocess.DEVNULL, capture_output=True)
    return p.returncode, p.stdout.decode("utf-8", "replace"), p.stderr.decode("utf-8", "replace"), time.monotonic() - t


def main():
    subprocess.run(["cargo", "build", "--release", "--example", "run_t", "--example", "exiftool-rs", "-j2"], cwd=here, capture_output=True)
    os.makedirs(work, exist_ok=True)
    env_perl = dict(os.environ)
    env_port = {k: v for k, v in os.environ.items() if k != "EXIFTOOL_LIB"}
    # no user config file on either side
    env_perl["EXIFTOOL_HOME"] = env_port["EXIFTOOL_HOME"] = os.path.join(work, "no_home")
    # (for scripts/full_modules/perl.sh: the tree to mount in its container)
    env_perl["EXIFTOOL_SRC"] = src
    os.makedirs(env_perl["EXIFTOOL_HOME"], exist_ok=True)
    env_host = dict(env_port, EXIFTOOL_WITHOUT=HOST_LACKS)
    same, bad, skipped, tp, tr = 0, [], [], 0.0, 0.0
    for n, (kind, args) in enumerate(cases(), 1):
        argv = [a.replace("{SRC}", src) for a in shlex.split(args)]
        stdin = None
        if len(argv) >= 2 and argv[-2] == "<":
            stdin, argv = argv[-1], argv[:-2]
        res = {}
        # (an S case is the program's own: run_t runs the script's)
        if kind == "S" and not os.environ.get("PORT_BIN"):
            skipped.append(f"{n}: {args}")
            continue
        # (PORT_BIN=path: the port's own exiftool binary, which reads no
        # script, instead of run_t running ExifTool's script)
        port_cmd = [os.environ["PORT_BIN"]] if os.environ.get("PORT_BIN") else [port, script]
        perl = perl_full_cmd if "M" in kind and not os.environ.get("PERL_CMD") else perl_cmd
        # against the host's Perl, the port does without the optional
        # modules that Perl lacks
        env = env_port if perl is not perl_cmd or os.environ.get("PERL_CMD") else env_host
        sides = (("perl", perl + [script], env_perl), ("port", port_cmd, env))
        for side, cmd, env in sides[1:] if kind == "S" else sides:
            d = os.path.join(work, str(n), side)
            prepare(d)
            cwd = d
            rc, out, err, secs = run(cmd + argv, cwd, env, stdin)
            same_files = "{SRC}" in args
            out, err = normalize(out, cwd, same_files), normalize(err, cwd, same_files)
            fl = files(d) if "W" in kind else {}
            with open(os.path.join(d, "..", side + ".txt"), "w") as f:
                f.write(f"rc={rc}\n--- stdout\n{out}--- stderr\n{err}--- files\n")
                f.writelines(f"{k} {v}\n" for k, v in sorted(fl.items()))
            res[side] = (rc, out, err, fl, secs)
        if kind == "S":
            tr += res["port"][4]
            if golden_check(args, *res["port"][:3]):
                same += 1
            else:
                bad.append(f"{n}: {args} (against scripts/cli_golden)")
            continue
        tp += res["perl"][4]
        tr += res["port"][4]
        if res["perl"][:4] == res["port"][:4]:
            same += 1
        else:
            bad.append(f"{n}: {args}")
    print(f"{same} identical, {len(bad)} different")
    for b in bad:
        print("different " + b)
    for s in skipped:
        print("skipped " + s + " (the port's own binary only: PORT_BIN)")
    gain = (tp - tr) / tp * 100 if tp else 0
    print(f"time: perl {tp:.1f} s, port {tr:.1f} s ({gain:+.0f} %)")


main()
