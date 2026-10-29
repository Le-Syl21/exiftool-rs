#!/usr/bin/env python3
"""What an ExifTool upgrade touches in the port: for each Perl file that
differs between two ExifTool trees, the named subs whose body changed, was
added or was removed. Every named sub of ExifTool runs as a Rust native in
the port (define_native("Pkg::Name") in src/, cli::define for the
`exiftool` script): the dumps hold the tables, the anonymous subs of the
tables and the module-level data, never a named sub. So each sub listed
must be ported by hand: a changed one in its Rust port, an added one as a
new native (it does not exist in the port until then: "NOT NATIVE"), and a
removed one unregistered (see docs/port/UPGRADING.md).

    upgrade_diff.py OLD_DIR NEW_DIR [--diff]

OLD_DIR and NEW_DIR are ExifTool source trees (with lib/ and exiftool).
--diff prints the unified diff of each changed sub. A sub is the lines from
its `sub NAME` line to the first `}` in column 0 after it, as ExifTool
writes them; the code between subs (tables, module-level statements) is
reported as "outside subs" with its count of changed lines."""
import difflib
import os
import re
import sys

here = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
args = [a for a in sys.argv[1:] if not a.startswith('--')]
if len(args) != 2:
    sys.exit(__doc__)
old_dir, new_dir = args
show_diff = '--diff' in sys.argv

native = set()
for root, _, files in os.walk(os.path.join(here, 'src')):
    for f in files:
        if f.endswith('.rs'):
            text = open(os.path.join(root, f), encoding='utf-8').read()
            native |= set(re.findall(r'define_native\(\s*"([^"]+)"', text))
            native |= set(re.findall(r'\bdefine\(\s*it,\s*"([^"]+)"', text))


def perl_files(d):
    """The Perl files of a tree, as paths relative to it."""
    out = {'exiftool'} if os.path.exists(os.path.join(d, 'exiftool')) else set()
    lib = os.path.join(d, 'lib')
    for root, _, files in os.walk(lib):
        for f in files:
            if f.endswith(('.pm', '.pl')):
                out.add(os.path.relpath(os.path.join(root, f), d))
    return out


def subs_of(path):
    """{full name: (first line, body lines)} and the lines outside subs."""
    if not os.path.exists(path):
        return {}, []
    lines = open(path, encoding='latin-1').read().split('\n')
    pkg = 'main'
    subs, outside = {}, []
    i = 0
    while i < len(lines):
        line = lines[i]
        if line.startswith('__END__'):
            break
        m = re.match(r'package\s+([\w:]+)\s*;', line)
        if m:
            pkg = m.group(1)
        m = re.match(r'sub\s+([\w:]+)\s*(\([^)]*\))?\s*(\{|$)', line)
        if m and not line.rstrip().endswith(';'):
            name = m.group(1) if '::' in m.group(1) else f'{pkg}::{m.group(1)}'
            j = i
            # (a one-line sub, `sub Name { ... }`, ends on its own line)
            if line.count('{') == 0 or line.count('{') != line.count('}'):
                j = i + 1
                while j < len(lines) and not lines[j].startswith('}'):
                    j += 1
            subs.setdefault(name, (i + 1, lines[i:j + 1]))
            i = j + 1
            continue
        outside.append(line)
        i += 1
    return subs, outside


todo = []
for rel in sorted(perl_files(old_dir) | perl_files(new_dir)):
    a, b = os.path.join(old_dir, rel), os.path.join(new_dir, rel)
    if os.path.exists(a) and os.path.exists(b) and open(a, 'rb').read() == open(b, 'rb').read():
        continue
    if '/Lang/' in rel or rel == 'lib/Image/ExifTool/TagLookup.pm':
        # data only (dumped)
        print(f'{rel}: data only (comes with the dumps)')
        continue
    old_subs, old_out = subs_of(a)
    new_subs, new_out = subs_of(b)
    n_old = sum(1 for _ in open(a, encoding='latin-1')) if os.path.exists(a) else 0
    n_new = sum(1 for _ in open(b, encoding='latin-1')) if os.path.exists(b) else 0
    out_changed = sum(1 for l in difflib.unified_diff(old_out, new_out, lineterm='', n=0)
                      if l[:1] in '+-' and not l.startswith(('+++', '---')))
    print(f'{rel}: {n_old} -> {n_new} lines, {out_changed} changed lines outside subs')
    for name in sorted(set(old_subs) | set(new_subs)):
        o, n = old_subs.get(name), new_subs.get(name)
        if o and n and o[1] == n[1]:
            continue
        kind = 'changed' if o and n else ('added' if n else 'removed')
        line = n[0] if n else o[0]
        state = 'NATIVE' if name in native else 'NOT NATIVE'
        print(f'  {kind:7} {state:10} {name} (line {line})')
        todo.append(name)
        if show_diff:
            for l in difflib.unified_diff(o[1] if o else [], n[1] if n else [], f'old {name}', f'new {name}', lineterm='', n=2):
                print('    ' + l)
print(f'-- {len(todo)} subs to port: ' + ', '.join(s.split('::')[-1] for s in todo))
