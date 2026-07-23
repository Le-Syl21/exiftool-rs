#!/usr/bin/env python3
"""Merge newly-emitted tags into every locales/*.yml, faithfully.

English descriptions come from ExifTool's own output (scripts collected them in
new81.tsv). Per-language translations come from ExifTool 13.59's Lang/*.pm via
lang_extract.pl. Tags ExifTool does not translate keep the English fallback,
matching how the existing files already carry English for untranslated tags.
Nothing is invented.
"""
import subprocess
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
LOCALES = REPO / "locales"
NEW_TSV = REPO / "scripts" / "new_tags_13.59.tsv"
EXTRACT = REPO / "scripts" / "lang_extract.pl"

# exiftool-rs locale code -> ExifTool Lang module name (None = English fallback)
LANG_MAP = {
    "en": None, "en_ca": "en_ca", "en_gb": "en_gb",
    "fr": "fr", "de": "de", "es": "es", "it": "it", "ja": "ja",
    "ko": "ko", "nl": "nl", "pl": "pl", "ru": "ru", "cs": "cs",
    "fi": "fi", "sk": "sk", "sv": "sv", "tr": "tr",
    "zh": "zh_cn", "zh_tw": "zh_tw",
    # No ExifTool Lang source -> English fallback:
    "ar": None, "bn": None, "hi": None, "pt": None,
}

def load_new():
    out = {}
    for line in NEW_TSV.read_text(encoding="utf-8").splitlines():
        if "\t" in line:
            k, v = line.split("\t", 1)
            out[k] = v
    return out

def lang_translations(langcode, names_file):
    if langcode is None:
        return {}
    res = subprocess.run(
        ["perl", str(EXTRACT), langcode, str(names_file)],
        capture_output=True, check=True,
    )
    d = {}
    for line in res.stdout.decode("utf-8").splitlines():
        if "\t" in line:
            k, v = line.split("\t", 1)
            d[k] = v
    return d

def yaml_quote(s):
    # Double-quoted YAML: escape backslash and double-quote; keep UTF-8 as-is.
    return '"' + s.replace("\\", "\\\\").replace('"', '\\"') + '"'

def is_tag_line(line):
    if not line or line[0] in "#_ \t\n":
        return False
    return ":" in line and line.split(":", 1)[0].replace("_", "").isalnum()

def main():
    new = load_new()
    names = list(new.keys())
    names_file = REPO / "scripts" / "new_tags_13.59.names"
    names_file.write_text("\n".join(names) + "\n", encoding="utf-8")
    total_before = None
    for yml in sorted(LOCALES.glob("*.yml")):
        code = yml.stem
        if code not in LANG_MAP:
            print(f"  SKIP unknown locale {code}")
            continue
        tr = lang_translations(LANG_MAP[code], names_file)

        lines = yml.read_text(encoding="utf-8").split("\n")
        # Trailing contiguous block of tag lines = the tag section.
        first_tag = None
        for i, ln in enumerate(lines):
            if is_tag_line(ln):
                first_tag = i
                break
        assert first_tag is not None, yml
        prefix = lines[:first_tag]
        tagblock = [ln for ln in lines[first_tag:] if ln.strip() != ""]
        existing = {}
        order = []
        for ln in tagblock:
            key = ln.split(":", 1)[0]
            existing[key] = ln
            order.append(key)

        added = 0
        for name in names:
            if name in existing:
                continue
            val = tr.get(name, new[name])
            existing[name] = f"{name}: {yaml_quote(val)}"
            added += 1

        keys_sorted = sorted(existing.keys())  # ASCII / C sort
        if total_before is None:
            total_before = len(order)
        # Update the count in the header comment lines (e.g. "3230" -> new total).
        new_total = len(keys_sorted)
        prefix = [ln.replace(str(total_before), str(new_total))
                  if ln.startswith("#") and str(total_before) in ln else ln
                  for ln in prefix]

        out = prefix + [existing[k] for k in keys_sorted] + [""]
        yml.write_text("\n".join(out), encoding="utf-8")
        n_tr = sum(1 for name in names if name in tr)
        print(f"  {code:6s} +{added} tags ({n_tr} translated, {added - n_tr} English fallback) -> {new_total}")
    names_file.unlink(missing_ok=True)

if __name__ == "__main__":
    main()
