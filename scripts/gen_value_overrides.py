#!/usr/bin/env python3
"""Generate group-scoped PrintConv value overrides from ExifTool's real output.

The flat tables (locales/values/<lang>.tsv) translate by (tag, value), which is
too coarse: the same tag name lives in several tables with different PrintConv
kinds, so ExifTool translates e.g. WhiteBalance "Auto" in [Canon]/[ExifIFD] but
not in [Nikon], and SelfTimer "Off" in some tables but not [Canon]. The family-1
group disambiguates these.

For every (group1, tag, english_value) the corpus exercises, this records what
ExifTool actually prints under -lang, but ONLY when it differs from what our flat
table would produce — so the override table stays minimal and precisely corrects:
  - suppression: flat translates but ExifTool keeps English  -> store English
  - realignment: ExifTool translates differently             -> store its string
  - composite:   flat lacks the joined value ExifTool builds  -> store its string
ExifTool is the sole oracle; nothing is inferred. System/ExifTool groups (volatile
file metadata, version) are excluded so run-to-run timestamps never leak in.
"""
import subprocess
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
VALUES = REPO / "locales" / "values"
EXIFTOOL = Path("/home/sylvain/dev/exiftool")
IMAGES = REPO / "tests" / "images"

LANGS = {
    "fr": "fr", "de": "de", "es": "es", "it": "it", "ja": "ja", "ko": "ko",
    "nl": "nl", "pl": "pl", "ru": "ru", "cs": "cs", "fi": "fi", "sk": "sk",
    "sv": "sv", "tr": "tr", "zh": "zh_cn", "zh_tw": "zh_tw",
    "en_ca": "en_ca", "en_gb": "en_gb",
}
EXCLUDE_GROUPS = {"System", "ExifTool"}
FILES = sorted(str(p) for p in IMAGES.iterdir() if p.is_file())


def run(lang_arg):
    """{(file, group1, tag): value} from `exiftool -s -G1 [-lang X]` over corpus."""
    cmd = ["./exiftool", "-s", "-G1", "-f"]
    if lang_arg:
        cmd += ["-lang", lang_arg]
    cmd += FILES
    out = subprocess.run(cmd, cwd=EXIFTOOL, capture_output=True).stdout.decode(
        "utf-8", "replace"
    )
    res = {}
    cur = None
    for line in out.splitlines():
        if line.startswith("========"):
            cur = line[8:].strip()
            continue
        if not line.startswith("[") or " : " not in line or cur is None:
            continue
        head, _, val = line.partition(" : ")
        rb = head.find("]")
        if rb < 0:
            continue
        group1 = head[1:rb]
        tag = head[rb + 1:].strip()
        if group1 in EXCLUDE_GROUPS:
            continue
        if tag and tag.isascii() and all(c.isalnum() or c == "_" for c in tag):
            res[(cur, group1, tag)] = val
    return res


def load_flat(code):
    flat = {}
    tsv = VALUES / f"{code}.tsv"
    for line in tsv.read_text(encoding="utf-8").splitlines():
        p = line.split("\t")
        if len(p) == 3:
            flat[(p[0], p[1])] = p[2]
    return flat


def main():
    print("ExifTool English baseline (-G1) ...", file=sys.stderr)
    english = run(None)
    grand = 0
    for code, lang_arg in LANGS.items():
        if not (VALUES / f"{code}.tsv").exists():
            continue
        flat = load_flat(code)
        loc = run(lang_arg)
        # Collect every ExifTool output observed for each (group1, tag, english).
        observed = {}           # key -> set(perl_output)
        for (f, g, tag), eng in english.items():
            perl_out = loc.get((f, g, tag))
            if perl_out is None:
                continue
            observed.setdefault((g, tag, eng), set()).add(perl_out)

        overrides = {}          # (group1, tag, eng) -> perl_output
        ambiguous = 0
        for (g, tag, eng), outs in observed.items():
            ours = flat.get((tag, eng), eng)   # what our flat table would print
            # Only emit an override when ExifTool is CONSISTENT for this key. If it
            # localizes the same (group, tag, value) differently across files (e.g.
            # an EXIF:IFD0 Orientation that comes through an XMP sidecar unconverted
            # in one file but through the normal PrintConv in another), the group is
            # not enough to tell them apart — keep the flat translation, which is
            # right for the majority, rather than suppress everyone from one outlier.
            if len(outs) != 1:
                if any(o != ours for o in outs):
                    ambiguous += 1
                continue
            perl_out = next(iter(outs))
            if perl_out == ours:
                continue                        # flat already agrees
            # A composite/add only counts if ExifTool actually localized it.
            if (tag, eng) not in flat and perl_out == eng:
                continue
            overrides[(g, tag, eng)] = perl_out
        conflicts = ambiguous
        lines = sorted(
            f"{g}\t{tag}\t{eng}\t{out}" for (g, tag, eng), out in overrides.items()
        )
        (VALUES / f"{code}.over.tsv").write_text(
            "\n".join(lines) + ("\n" if lines else ""), encoding="utf-8"
        )
        grand += len(lines)
        note = f" ({conflicts} conflicts skipped)" if conflicts else ""
        print(f"  {code:6s} {len(lines)} overrides{note}")
    print(f"total overrides: {grand}", file=sys.stderr)


if __name__ == "__main__":
    main()
