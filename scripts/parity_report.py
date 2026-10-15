#!/usr/bin/env python3
"""Turn `parity --dump` TSV into a self-contained parity-report web page.

Reads the TSV (stdin or argv[1]) produced by `cargo run --features parity
--bin parity -- --dump`, and writes a single static HTML page to stdout: a
summary of exiftool-rs vs Perl ExifTool over the whole shipped test corpus,
then one collapsible section per file with a tag-by-tag table and an ISO column.

When the dump carries a write section (`parity --dump` always does), a "Write"
section follows: every corpus file exiftool-rs writes, edited the same way by
both tools and verified by re-reading the outputs, summarised per format and
detailed per file and case.

    parity --dump | python3 scripts/parity_report.py \
        --rs 0.7.3 --exiftool 13.59 > index.html
"""
import argparse
import html
import sys
from collections import OrderedDict

HEADER = "file\ttag\texiftool\texiftool_rs\tiso"
WRITE_HEADER = ("WRITE\tfile\tformat\tcase\tedits\tours\tperl\texpected"
                "\tperl_iso\treread\tvalid\tverdict")


def read_rows(text):
    files = OrderedDict()  # file -> list[(tag, perl, ours, iso)]
    write = {"cases": [], "details": {}, "ignored": [], "time": None}
    rs_ver = et_ver = ""
    started = False
    in_write = False
    for line in text.splitlines():
        if line == WRITE_HEADER:
            in_write = started = True
            continue
        if in_write:
            read_write_line(line, write)
            continue
        if not started:
            # Preamble printed by the binary before the TSV header.
            if line.startswith("Perl ExifTool:"):
                et_ver = line.split(":", 1)[1].split("(")[0].strip()
            elif line.startswith("exiftool-rs:"):
                rs_ver = line.split(":", 1)[1].split("(")[0].strip()
            if line == HEADER:
                started = True
            continue
        parts = line.split("\t")
        if len(parts) != 5:
            continue
        f, tag, perl, ours, iso = parts
        files.setdefault(f, []).append((tag, perl, ours, iso == "1"))
    return files, write, rs_ver, et_ver


CASE_FIELDS = ("file", "format", "case", "edits", "ours", "perl", "expected",
               "perl_iso", "reread", "valid", "verdict")


def read_write_line(line, write):
    parts = line.split("\t")
    kind, rest = parts[0], parts[1:]
    if kind == "C" and len(rest) == len(CASE_FIELDS):
        write["cases"].append(dict(zip(CASE_FIELDS, rest)))
    elif kind == "D" and len(rest) == 6:
        f, case, check, key, ours, ref = rest
        write["details"].setdefault((f, case), []).append((check, key, ours, ref))
    elif kind == "I" and len(rest) == 2:
        write["ignored"].append(tuple(rest))
    elif kind == "T" and len(rest) == 3:
        write["time"] = (float(rest[0]), int(rest[1]), int(rest[2]))


def esc(s):
    return html.escape(s, quote=False)


# Verdicts, most severe first, with the label and pill class shown for them.
VERDICTS = OrderedDict([
    ("silent", ("silent no-op", "bad")),
    ("refused", ("refused", "warn")),
    ("broken", ("broken file", "bad")),
    ("mismatch", ("mismatch", "warn")),
    ("noref", ("no Perl reference", "info")),
    ("both-refuse", ("neither writes", "info")),
    ("iso", ("ISO", "good")),
])

CHECK_LABELS = {
    "expected": "expected state",
    "perl-identical": "Perl-identical",
    "reread": "re-read consistent",
    "valid": "valid file",
    "silent": "silent no-op",
}


def mark(v):
    if v in ("1", "ok"):
        return '<td class="c ok">✓</td>'
    if v in ("-", ""):
        return '<td class="c na">–</td>'
    return f'<td class="c ko">✗{"" if v in ("0",) else " " + esc(v)}</td>'


def pill(verdict):
    label, cls = VERDICTS.get(verdict, (verdict, "warn"))
    return f'<span class="pill {cls}">{esc(label)}</span>'


def render_write(write):
    cases = write["cases"]
    if not cases:
        return ""
    out = []
    w = out.append
    count = lambda v, rows: sum(1 for c in rows if c["verdict"] == v)
    total = len(cases)
    iso = count("iso", cases)
    mism = count("mismatch", cases) + count("broken", cases) + count("refused", cases)
    broken = count("broken", cases) + count("refused", cases)
    silent = count("silent", cases)
    noref = count("noref", cases) + count("both-refuse", cases)
    nfiles = len({c["file"] for c in cases})
    t = write["time"]
    w('<section class="write" id="write">')
    w('<p class="eyebrow">Write parity</p>'
      '<h2>Every write verified by re-reading</h2>'
      '<p class="sub">Each corpus file exiftool-rs writes gets the same small set of '
      'edits from both tools — set an existing tag, add one, delete one, delete a '
      'whole group (<code>-EXIF:all=</code>, <code>-XMP:all=</code>, '
      '<code>-IPTC:all=</code>, <code>-GPS:all=</code>) and a mixed edit. Both '
      'outputs are re-read by Perl ExifTool, the independent referee, and ours also '
      'by exiftool-rs. A case is ISO when the requested state is there, the tags '
      'equal those of Perl\'s own output, our reader agrees with Perl on our file '
      'and Perl reports no new error or warning.</p>')
    w('<div class="tiles">'
      f'<div class="tile"><span class="n">{total}</span><span class="l">cases on {nfiles} files</span></div>'
      f'<div class="tile {"good" if iso == total - noref else ""}"><span class="n">{iso}</span><span class="l">ISO</span></div>'
      f'<div class="tile {"good" if mism == 0 else "warn"}"><span class="n">{mism}</span><span class="l">mismatches</span></div>'
      f'<div class="tile {"good" if silent == 0 else "bad"}"><span class="n">{silent}</span><span class="l">silent no-ops</span></div>'
      f'<div class="tile"><span class="n">{noref}</span><span class="l">no Perl reference</span></div>'
      '</div>')
    w(f'<p class="meta">Mismatches include {broken} refused writes or broken files. '
      '<strong>Silent no-op</strong> = <code>write_info</code> returned <code>Ok</code> '
      'but the output is byte-identical or a requested change is absent while Perl '
      'made it. <strong>No Perl reference</strong> = Perl ExifTool does not write '
      'that format; our edits are still verified on their own.'
      + (f' · matrix ran in {t[0]:.0f} s' if t else "") + '</p>')

    # Per-format summary table.
    formats = OrderedDict()
    for c in sorted(cases, key=lambda c: c["format"]):
        formats.setdefault(c["format"], []).append(c)
    w('<div class="panel scroll"><table><thead><tr><th>Format</th><th class="c">Files</th>'
      '<th class="c">Cases</th><th class="c">ISO</th><th class="c">Mismatch</th>'
      '<th class="c">Silent</th><th class="c">Refused</th><th class="c">Broken</th>'
      '<th class="c">No ref</th></tr></thead><tbody>')
    for fmt, rows in formats.items():
        m = count("mismatch", rows)
        sl = count("silent", rows)
        nr = count("noref", rows) + count("both-refuse", rows)
        cls = ' class="d"' if sl else ""
        w(f'<tr{cls}><td class="t"><a href="#w-{esc(fmt)}">{esc(fmt)}</a></td>'
          f'<td class="c">{len({c["file"] for c in rows})}</td><td class="c">{len(rows)}</td>'
          f'<td class="c">{count("iso", rows)}</td><td class="c">{m}</td>'
          f'<td class="c">{sl}</td><td class="c">{count("refused", rows)}</td>'
          f'<td class="c">{count("broken", rows)}</td><td class="c">{nr}</td></tr>')
    w('</tbody></table></div>')

    # Per format, per file: the case table, with differing tags under each case.
    for fmt, rows in formats.items():
        bad = sum(1 for c in rows if c["verdict"] not in ("iso", "noref", "both-refuse"))
        badge = pill("iso") if bad == 0 else f'<span class="pill warn">{bad} failing</span>'
        w(f'<details class="fmt" id="w-{esc(fmt)}"><summary><span class="fname">{esc(fmt)}</span>'
          f'<span class="cnt">{len(rows)} cases</span>{badge}</summary><div class="inner">')
        per_file = OrderedDict()
        for c in rows:
            per_file.setdefault(c["file"], []).append(c)
        for f, frows in per_file.items():
            fbad = sum(1 for c in frows if c["verdict"] not in ("iso", "noref", "both-refuse"))
            worst = min((c["verdict"] for c in frows), key=lambda v: list(VERDICTS).index(v)
                        if v in VERDICTS else 0)
            w(f'<details{" open" if fbad else ""}><summary><span class="fname">{esc(f)}</span>'
              f'<span class="cnt">{len(frows)} cases</span>{pill(worst)}</summary>')
            w('<div class="scroll"><table><thead><tr><th>Case</th><th>Edits</th>'
              '<th>exiftool-rs</th><th>ExifTool</th><th class="c">Expected</th>'
              '<th class="c">Perl-identical</th><th class="c">Re-read</th>'
              '<th class="c">Valid</th><th>Verdict</th></tr></thead><tbody>')
            for c in frows:
                cls = "" if c["verdict"] in ("iso", "noref", "both-refuse") else ' class="d"'
                w(f'<tr{cls}><td class="t">{esc(c["case"])}</td><td>{esc(c["edits"])}</td>'
                  f'<td>{esc(c["ours"])}</td><td>{esc(c["perl"])}</td>'
                  f'{mark(c["expected"])}{mark(c["perl_iso"])}{mark(c["reread"])}'
                  f'{mark(c["valid"])}<td>{pill(c["verdict"])}</td></tr>')
                det = write["details"].get((c["file"], c["case"]))
                if det:
                    w('<tr class="sub"><td></td><td colspan="8"><table class="det"><thead><tr>'
                      '<th>Check</th><th>Tag / edit</th><th>Our output</th>'
                      '<th>Reference</th></tr></thead><tbody>')
                    for check, key, ours, ref in det:
                        w(f'<tr><td>{esc(CHECK_LABELS.get(check, check))}</td>'
                          f'<td class="t">{esc(key)}</td><td>{esc(ours)}</td>'
                          f'<td>{esc(ref)}</td></tr>')
                    w('</tbody></table></td></tr>')
            w('</tbody></table></div></details>')
        w('</div></details>')

    if write["ignored"]:
        w('<details class="fmt"><summary><span class="fname">Tags left out of the '
          'comparison</span><span class="cnt">'
          f'{len(write["ignored"])} tags</span></summary><div class="scroll"><table>'
          '<thead><tr><th>Tag</th><th>Why</th></tr></thead><tbody>')
        for tag, why in write["ignored"]:
            w(f'<tr><td class="t">{esc(tag)}</td><td>{esc(why)}</td></tr>')
        w('</tbody></table></div></details>')
    w('<p class="meta">Reference: the reference column holds the requested '
      'value for an expected-state miss, Perl\'s re-read of its own output for a '
      'Perl-identical miss, and Perl\'s re-read of our output for a re-read miss. '
      'Reader deltas that already exist on the unedited file are not counted again.</p>')
    w('</section>')
    return "".join(out)


def render(files, write, rs_ver, et_ver):
    total_tags = sum(len(v) for v in files.values())
    total_delta = sum(1 for v in files.values() for r in v if not r[3])
    clean_files = sum(1 for v in files.values() if all(r[3] for r in v))
    pct = 100.0 * (total_tags - total_delta) / total_tags if total_tags else 100.0

    out = []
    w = out.append
    w(f"""<!doctype html><html lang="en"><head>
<meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1">
<title>exiftool-rs — ExifTool in pure Rust, tag-by-tag parity with ExifTool {esc(et_ver)}</title>
<meta name="description" content="exiftool-rs is a pure Rust reimplementation of ExifTool: read, write and edit EXIF, XMP, IPTC and MakerNote metadata. This report compares every tag it extracts with Perl ExifTool {esc(et_ver)}, regenerated by CI.">
<link rel="canonical" href="https://le-syl21.github.io/exiftool-rs/">
<meta name="google-site-verification" content="TqbXre6qrm9jaoj6tFwRRiI2vuQilAZLm6kUJA-etmo">
<meta property="og:type" content="website">
<meta property="og:title" content="exiftool-rs reads the same as ExifTool {esc(et_ver)}">
<meta property="og:description" content="Pure Rust reimplementation of ExifTool. Every tag of the test corpus compared with Perl ExifTool, tag by tag.">
<meta property="og:url" content="https://le-syl21.github.io/exiftool-rs/">
<meta property="og:image" content="https://raw.githubusercontent.com/Le-Syl21/exiftool-rs/main/assets/screenshot.png">
<meta name="twitter:card" content="summary_large_image">
<script type="application/ld+json">{{"@context":"https://schema.org","@type":"SoftwareSourceCode","name":"exiftool-rs","description":"Pure Rust reimplementation of ExifTool: read, write and edit image, audio, video and document metadata.","codeRepository":"https://github.com/Le-Syl21/exiftool-rs","programmingLanguage":"Rust","license":"https://www.gnu.org/licenses/gpl-3.0.html"}}</script>
<style>{CSS}</style></head><body>
<header class="hero">
  <p class="eyebrow">Live differential parity</p>
  <h1>exiftool-rs reads the same as ExifTool</h1>
  <p class="sub">Every tag exiftool-rs extracts from the shipped test corpus, compared
     tag-by-tag against Perl <strong>ExifTool {esc(et_ver)}</strong> — the exact
     bytes both tools print for <code>-s -G1</code>. Every write is then checked by
     re-reading it (<a href="#write">Write parity</a>). Regenerated by CI on every release.</p>
  <div class="tiles">
    <div class="tile"><span class="n">{clean_files}/{len(files)}</span><span class="l">files identical</span></div>
    <div class="tile"><span class="n">{total_tags:,}</span><span class="l">tags compared</span></div>
    <div class="tile {'good' if total_delta==0 else 'warn'}"><span class="n">{total_delta}</span><span class="l">deltas</span></div>
    <div class="tile"><span class="n">{pct:.2f}%</span><span class="l">identical</span></div>
  </div>
  <p class="meta">exiftool-rs {esc(rs_ver)} vs ExifTool {esc(et_ver)} · machine-state tags
     (dates, permissions, tool version) excluded · {len(files)} files</p>
</header>
<main>""")

    w(render_write(write))
    if write["cases"]:
        w('<h2 class="readh">Read parity, file by file</h2>')
    for f, rows in files.items():
        deltas = sum(1 for r in rows if not r[3])
        ok = deltas == 0
        badge = '<span class="pill good">ISO</span>' if ok else f'<span class="pill warn">{deltas} Δ</span>'
        w(f'<details{"" if ok else " open"}><summary>'
          f'<span class="fname">{esc(f)}</span>'
          f'<span class="cnt">{len(rows)} tags</span>{badge}</summary>')
        w('<div class="scroll"><table><thead><tr><th>Group1:Tag</th>'
          '<th>ExifTool</th><th>exiftool-rs</th><th class="c">ISO</th></tr></thead><tbody>')
        for tag, perl, ours, iso in rows:
            cls = "" if iso else ' class="d"'
            mark = "✓" if iso else "✗"
            w(f'<tr{cls}><td class="t">{esc(tag)}</td><td>{esc(perl)}</td>'
              f'<td>{esc(ours)}</td><td class="c">{mark}</td></tr>')
        w("</tbody></table></div></details>")

    w("<script>function o(){var e=location.hash&&document.getElementById(location.hash.slice(1));"
      "if(e&&e.tagName==='DETAILS')e.open=true}addEventListener('hashchange',o);o()</script>")
    w("</main><footer>Generated from <code>parity --dump</code> · "
      "<a href=\"https://github.com/Le-Syl21/exiftool-rs\">Le-Syl21/exiftool-rs</a></footer>"
      "</body></html>")
    return "".join(out)


CSS = """
:root{
  --bg:#f7f8f6; --panel:#ffffff; --ink:#1a1e1b; --muted:#5c655e; --line:#e2e6e1;
  --accent:#1f8a4c; --accent-soft:#e5f3ea; --warn:#b23b3b; --warn-soft:#f6e5e5;
  --bad:#8f1d1d; --bad-soft:#f3d6d6;
  --mono:ui-monospace,"SFMono-Regular",Menlo,Consolas,monospace;
  --sans:system-ui,-apple-system,"Segoe UI",Roboto,sans-serif;
}
@media (prefers-color-scheme:dark){:root{
  --bg:#0f130f; --panel:#171c17; --ink:#e7ece7; --muted:#9aa89d; --line:#28302a;
  --accent:#4cc47f; --accent-soft:#15271d; --warn:#e07575; --warn-soft:#2a1717;
  --bad:#ff8a8a; --bad-soft:#3a1414;
}}
:root[data-theme="light"]{--bg:#f7f8f6;--panel:#fff;--ink:#1a1e1b;--muted:#5c655e;--line:#e2e6e1;--accent:#1f8a4c;--accent-soft:#e5f3ea;--warn:#b23b3b;--warn-soft:#f6e5e5;--bad:#8f1d1d;--bad-soft:#f3d6d6;}
:root[data-theme="dark"]{--bg:#0f130f;--panel:#171c17;--ink:#e7ece7;--muted:#9aa89d;--line:#28302a;--accent:#4cc47f;--accent-soft:#15271d;--warn:#e07575;--warn-soft:#2a1717;--bad:#ff8a8a;--bad-soft:#3a1414;}
*{box-sizing:border-box}
body{margin:0;background:var(--bg);color:var(--ink);font-family:var(--sans);line-height:1.5;
  font-variant-numeric:tabular-nums}
.hero{max-width:70rem;margin:0 auto;padding:3.5rem 1.5rem 2rem}
.eyebrow{font-family:var(--mono);font-size:.75rem;letter-spacing:.14em;text-transform:uppercase;
  color:var(--accent);margin:0 0 .5rem}
h1{font-size:clamp(1.8rem,4vw,2.7rem);line-height:1.1;text-wrap:balance;margin:0 0 .6rem;font-weight:650}
.sub{max-width:46rem;color:var(--muted);font-size:1.05rem;margin:0 0 1.8rem}
.sub code,.meta code{font-family:var(--mono);font-size:.9em}
.tiles{display:grid;grid-template-columns:repeat(auto-fit,minmax(9rem,1fr));gap:.9rem;margin:0 0 1rem}
.tile{background:var(--panel);border:1px solid var(--line);border-radius:12px;padding:1rem 1.1rem;
  display:flex;flex-direction:column;gap:.15rem}
.tile .n{font-size:1.7rem;font-weight:650;font-family:var(--mono)}
.tile .l{font-size:.8rem;color:var(--muted)}
.tile.good{border-color:var(--accent);background:var(--accent-soft)}
.tile.good .n{color:var(--accent)}
.tile.warn{border-color:var(--warn);background:var(--warn-soft)}
.tile.warn .n{color:var(--warn)}
.meta{color:var(--muted);font-size:.82rem;margin:.4rem 0 0}
main{max-width:70rem;margin:0 auto;padding:1rem 1.5rem 3rem;display:flex;flex-direction:column;gap:.5rem}
details{background:var(--panel);border:1px solid var(--line);border-radius:10px;overflow:hidden}
summary{display:flex;align-items:center;gap:.8rem;padding:.7rem 1rem;cursor:pointer;
  font-family:var(--mono);font-size:.9rem;list-style:none}
summary::-webkit-details-marker{display:none}
summary::before{content:"▸";color:var(--muted);font-size:.8rem}
details[open] summary::before{content:"▾"}
.fname{font-weight:600}
.cnt{color:var(--muted);font-size:.8rem;margin-left:auto}
.pill{font-size:.72rem;padding:.12rem .5rem;border-radius:99px;font-weight:600}
.pill.good{background:var(--accent-soft);color:var(--accent)}
.pill.warn{background:var(--warn-soft);color:var(--warn)}
.scroll{overflow-x:auto;border-top:1px solid var(--line)}
table{border-collapse:collapse;width:100%;font-family:var(--mono);font-size:.82rem}
th,td{text-align:left;padding:.35rem .8rem;border-bottom:1px solid var(--line);
  white-space:nowrap;max-width:32rem;overflow:hidden;text-overflow:ellipsis}
th{position:sticky;top:0;background:var(--panel);color:var(--muted);font-weight:600;
  font-size:.72rem;letter-spacing:.05em;text-transform:uppercase}
td.t{color:var(--accent)}
td.c,th.c{text-align:center;width:3rem}
tr.d td{background:var(--warn-soft)}
tr.d td.c{color:var(--warn);font-weight:700}
footer{max-width:70rem;margin:0 auto;padding:2rem 1.5rem 3rem;color:var(--muted);
  font-size:.82rem;border-top:1px solid var(--line)}
a{color:var(--accent)}
h2{font-size:1.5rem;line-height:1.2;margin:0 0 .6rem;font-weight:650;text-wrap:balance}
.write{display:flex;flex-direction:column;gap:.5rem;margin:0 0 2rem}
.write .sub{margin:0 0 1rem}
.readh{margin:1rem 0 .4rem}
.panel{background:var(--panel);border:1px solid var(--line);border-radius:10px;border-top:1px solid var(--line)}
.fmt .inner{display:flex;flex-direction:column;gap:.4rem;padding:.5rem;border-top:1px solid var(--line)}
.tile.bad{border-color:var(--bad);background:var(--bad-soft)}
.tile.bad .n{color:var(--bad)}
.pill.bad{background:var(--bad);color:var(--panel)}
.pill.info{background:var(--line);color:var(--muted)}
td.ok{color:var(--accent)} td.ko{color:var(--warn);font-weight:700} td.na{color:var(--muted)}
tr.sub td{background:var(--bg);white-space:normal}
table.det{font-size:.76rem;margin:.2rem 0}
table.det th{position:static;background:transparent}
table.det td{white-space:normal;max-width:28rem;word-break:break-word}
"""


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("tsv", nargs="?", help="TSV file (default: stdin)")
    ap.add_argument("--rs", default="", help="exiftool-rs version")
    ap.add_argument("--exiftool", default="", help="ExifTool version")
    a = ap.parse_args()
    text = open(a.tsv, encoding="utf-8").read() if a.tsv else sys.stdin.read()
    files, write, rs_ver, et_ver = read_rows(text)
    if not files:
        sys.exit("no data rows found in TSV (missing header?)")
    # Explicit flags override the versions parsed from the dump preamble.
    sys.stdout.write(render(files, write, a.rs or rs_ver, a.exiftool or et_ver))


if __name__ == "__main__":
    main()
