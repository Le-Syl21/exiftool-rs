#!/usr/bin/env python3
"""How much of ExifTool is ported: for each Perl file, its named subs and
how many are registered as Rust natives (define_native("Pkg::Name") in
src/modules/).
    port_status.py [EXIFTOOL_LIB] [FILE...]"""
import os, re, sys

here = os.path.dirname(os.path.abspath(__file__))
lib = sys.argv[1] if len(sys.argv) > 1 else os.path.join(here, '../../../exiftool/lib')
only = sys.argv[2:]
native = set()
for root, _, files in os.walk(os.path.join(here, '../src/modules')):
    for f in files:
        if f.endswith('.rs'):
            native |= set(re.findall(r'define_native\(\s*"([^"]+)"', open(os.path.join(root, f)).read()))

rows = []
for root, _, files in os.walk(lib):
    for f in files:
        if not f.endswith(('.pm', '.pl')) or '/Lang' in root or '/Charset' in root:
            continue
        path = os.path.join(root, f)
        rel = os.path.relpath(path, lib)
        if only and not any(rel.endswith(o) for o in only):
            continue
        pkg, subs = 'main', []
        for line in open(path, encoding='latin-1'):
            m = re.match(r'package\s+([\w:]+)\s*;', line)
            if m:
                pkg = m.group(1)
            m = re.match(r'sub\s+([\w:]+)\s*(\([^)]*\))?\s*(\{|$)', line)
            if m and not line.rstrip().endswith(';'):
                name = m.group(1) if '::' in m.group(1) else f'{pkg}::{m.group(1)}'
                subs.append(name)
        if subs:
            done = [s for s in subs if s in native]
            rows.append((rel, len(subs), len(done), [s for s in subs if s not in native]))
rows.sort()
tot = sum(r[1] for r in rows)
ported = sum(r[2] for r in rows)
for rel, n, d, _ in rows:
    if d or only:
        print(f'{rel:45} {d:4}/{n:<4}')
print(f'-- {ported} of {tot} subs ported ({100*ported/max(tot,1):.1f} %), {sum(1 for r in rows if r[1]==r[2])} of {len(rows)} files complete')
if only:
    for rel, n, d, todo in rows:
        if todo:
            print(f'{rel} still interpreted: ' + ', '.join(t.split('::')[-1] for t in todo))
