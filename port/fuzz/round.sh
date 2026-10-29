#!/bin/sh
# One fuzzing round of 15 minutes on TARGET, three processes in fork mode
# (which collects every crash instead of stopping at the first), without a
# sanitizer (Rust is memory-safe: the panics, overflows, hangs and memory
# blow-ups are what is looked for, and the interpreter's start is slow
# enough). The artifacts go to artifacts/TARGET-rN/, the log to
# artifacts/TARGET-rN.log; then triage.py groups them by cause.
#   sh round.sh TARGET N
set -eu
cd "$(dirname "$0")"
t=$1 n=$2
mkdir -p "artifacts/$t-r$n" "corpus/$t"
CARGO_BUILD_JOBS=2 timeout 1000 cargo +nightly fuzz run -s none "$t" -- -fork=3 \
    -ignore_crashes=1 -ignore_ooms=1 -ignore_timeouts=1 -max_total_time=900 \
    -rss_limit_mb=2048 -timeout=10 -artifact_prefix="artifacts/$t-r$n/" \
    > "artifacts/$t-r$n.log" 2>&1 || true
grep -E '^#[0-9]+: cov' "artifacts/$t-r$n.log" | tail -1
echo "$(ls "artifacts/$t-r$n" | wc -l) artifacts in artifacts/$t-r$n"
