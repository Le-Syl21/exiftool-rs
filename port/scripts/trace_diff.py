#!/usr/bin/env python3
"""Compare two call traces (scripts/trace_db.pl format) as multisets of
calls, ignoring order: prints the perl calls the port did not make (or made
with another result), in perl's order.
    trace_diff.py perl.txt port.txt [N]"""
import sys
from collections import Counter

def records(path):
    out, stack = [], []
    for line in open(path, encoding='latin-1'):
        s = line.strip('\n').lstrip(' ')
        if s.startswith('> '):
            name, _, args = s[2:].partition(' ')
            stack.append((name, args))
        elif s.startswith('< ') and stack:
            name, args = stack.pop()
            res = s[2:]
            # Hash order differs between perl and the port: compare lists
            # of results as sets.
            if '|' in res:
                res = '|'.join(sorted(res.split('|')))
            out.append((name, args, res))
    return out

perl, port = records(sys.argv[1]), records(sys.argv[2])
n = int(sys.argv[3]) if len(sys.argv) > 3 else 25
have = Counter(port)
by_call = {}
for r in port:
    by_call.setdefault((r[0], r[1]), set()).add(r[2])
shown = 0
calls = Counter((r[0], r[1]) for r in port)
for r in perl:
    if have[r] > 0:
        have[r] -= 1
        calls[(r[0], r[1])] -= 1
        continue
    # A call in void context returns nothing to compare.
    if (r[2] == 'void' or 'void' in by_call.get((r[0], r[1]), ())) and calls[(r[0], r[1])] > 0:
        calls[(r[0], r[1])] -= 1
        continue
    other = by_call.get((r[0], r[1]))
    print(f"{r[0]}({r[1][:150]})\n    perl: {r[2][:200]}\n    port: {'; '.join(list(other)[:3])[:200] if other else '(not called)'}")
    shown += 1
    if shown >= n:
        break
print(f"-- {len(perl)} perl calls, {len(port)} port calls")
