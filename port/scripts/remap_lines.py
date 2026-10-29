#!/usr/bin/env python3
"""Move the Perl line numbers that the natives give pl::at(N) (and the local
helpers that pass a line on to it, LINE_SITES below) from one
ExifTool release to the next. A native sets the line of the Perl statement
it runs (for the locations of perl's messages, " at FILE line N."), so when
a release adds or removes lines above a ported sub, its pl::at numbers must
follow.

    remap_lines.py OLD_DIR NEW_DIR [--write]

For each Rust file of src/, the Perl files of the natives it registers are
looked up (src/runtime/sub_index.rs must still be the OLD release's, or is
regenerated from OLD_DIR here: the script reads the subs itself), and each
pl::at(N) is attributed to the one of those files where line N lies inside a
sub that the Rust file registers. N is then mapped through the diff of the
OLD and NEW versions of that file. Lines that the diff changed are listed
for a manual check (their statement was edited: the port of that change
sets the right number). Without --write, only prints what would change."""
import difflib
import os
import re
import sys

here = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
args = [a for a in sys.argv[1:] if not a.startswith('--')]
if len(args) != 2:
    sys.exit(__doc__)
old_dir, new_dir = args
write = '--write' in sys.argv


def subs_of(path):
    """{full name: (first line, last line)} of a Perl file (1-based)."""
    lines = open(path, encoding='latin-1').read().split('\n')
    pkg, subs = 'main', {}
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
            if line.count('{') == 0 or line.count('{') != line.count('}'):
                j = i + 1
                while j < len(lines) and not lines[j].startswith('}'):
                    j += 1
            subs.setdefault(name, (i + 1, j + 1))
            i = j + 1
            continue
        i += 1
    return subs


def perl_files(d):
    out = ['exiftool']
    for root, _, files in os.walk(os.path.join(d, 'lib')):
        for f in files:
            if f.endswith(('.pm', '.pl')) and '/Lang' not in root and '/Charset' not in root:
                out.append(os.path.relpath(os.path.join(root, f), d))
    return out


# where each sub of OLD is
sub_file = {}
file_subs = {}
for rel in perl_files(old_dir):
    s = subs_of(os.path.join(old_dir, rel))
    file_subs[rel] = s
    for name, rng in s.items():
        sub_file.setdefault(name, (rel, rng))

maps = {}


def line_map(rel):
    """old line -> new line (None where the line itself changed)."""
    if rel in maps:
        return maps[rel]
    a = open(os.path.join(old_dir, rel), encoding='latin-1').read().split('\n')
    b_path = os.path.join(new_dir, rel)
    b = open(b_path, encoding='latin-1').read().split('\n') if os.path.exists(b_path) else []
    m = {}
    for tag, i1, i2, j1, j2 in difflib.SequenceMatcher(None, a, b, autojunk=False).get_opcodes():
        for k in range(i1, i2):
            m[k + 1] = (j1 + (k - i1) + 1) if tag == 'equal' else None
    maps[rel] = m
    return m


# the places that hold a Perl line: pl::at(N), and the local helpers that
# take the line of their caller's statement and pass it on to pl::at
LINE_SITES = re.compile('|'.join([
    r'pl::at\((\d+)\)',
    r'pl::at\(if [^{}]*\{ (\d+) \} else \{ (\d+) \}\)',
    r'foreach_items\([^;]*?, (\d+)\)',
    r'set_rational\(it, (\d+),',
    r'et_suffix\(it, (\d+)\)',
    r'located\(it, (\d+),',
]))
total = changed = manual = 0
for root, _, files in os.walk(os.path.join(here, 'src')):
    for f in sorted(files):
        if not f.endswith('.rs'):
            continue
        path = os.path.join(root, f)
        text = open(path, encoding='utf-8').read()
        names = set(re.findall(r'define_native\(\s*"([^"]+)"', text))
        names |= set(re.findall(r'\bdefine\(\s*it,\s*"([^"]+)"', text))
        ranges = [sub_file[n] for n in names if n in sub_file]
        if '/cli/' in path:
            # the script's main program is a native too (main::<program>)
            ranges.append(('exiftool', (1, 10 ** 9)))
        rel_path = os.path.relpath(path, here)

        def one(n_text, whole):
            global total, changed, manual
            n = int(n_text)
            total += 1
            cands = sorted({rel for rel, (lo, hi) in ranges if lo <= n <= hi})
            if len(cands) != 1:
                print(f'{rel_path}: {whole}: {"no" if not cands else "several"} Perl file(s) {cands}, left as is')
                manual += 1
                return n_text
            new = line_map(cands[0]).get(n)
            if new is None:
                print(f'{rel_path}: {whole} ({cands[0]}): the line changed, check by hand')
                manual += 1
                return n_text
            if new != n:
                changed += 1
            return str(new)

        def repl(m):
            out, last = [], m.start()
            for g in range(1, len(m.groups()) + 1):
                if m.group(g) is None:
                    continue
                out.append(text[last:m.start(g)])
                out.append(one(m.group(g), m.group(0)))
                last = m.end(g)
            out.append(text[last:m.end()])
            return ''.join(out)

        out = LINE_SITES.sub(repl, text)
        if write and out != text:
            open(path, 'w', encoding='utf-8').write(out)
print(f'-- {total} line sites, {changed} moved, {manual} to check by hand' + ('' if write else ' (dry run)'))
