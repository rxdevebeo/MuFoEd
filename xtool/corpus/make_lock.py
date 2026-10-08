#!/usr/bin/env python3
"""Generate `testdata-lock/cc0.toml` from the three CC0 manifests.

`docs/CC0_CORPUS_MIGRATION_PLAN.md` §4.1. The corpus bytes are never committed;
the lock file is: one `[[doc]]` per document with a stable id, the path under
`testdata/`, the SHA-256 and size, the archive.org URL, the licence, the tiers
and a structural feature scan.

The three manifests use different schemas (`CC0` names the file
`local_filename`, the other two `filename`); this script is the one place that
knows that. Every document must be present under `--root` with the hash and
size its manifest records, because the feature scan reads the package itself.

The output is deterministic: documents are sorted by id, features are sorted,
and nothing time- or machine-dependent is written. Tier membership is the
`CI_CORE` list below (§5); changing a tier means editing that list and
regenerating, never editing the lock by hand.

Usage:
    python3 xtool/corpus/make_lock.py --root /path/to/testdata [--out testdata-lock/cc0.toml]
    python3 xtool/corpus/make_lock.py --root ... --check   # exit 1 if the lock is stale
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import re
import sys
import zipfile

HERE = os.path.dirname(os.path.abspath(__file__))
REPO = os.path.dirname(os.path.dirname(HERE))
DEFAULT_OUT = os.path.join(REPO, "testdata-lock", "cc0.toml")

# (directory under testdata/, id prefix, file-name field in its manifest)
CORPORA = (
    ("CC0", "cc0", "local_filename"),
    ("CC0_DOCX", "cc0-docx", "filename"),
    ("CC0_DOCX_1", "cc0-docx-1", "filename"),
)

LICENSE_URLS = {
    "http://creativecommons.org/publicdomain/zero/1.0/",
    "https://creativecommons.org/publicdomain/zero/1.0/",
}

# The ci-core tier (plan §5): 26 documents chosen by greedy set cover over the
# feature scan, plus the two CC0 documents `p10_media` already pins.
CI_CORE = (
    "cc0-docx-1/076",  # the only pctPosVOffset; wpg:wgp
    "cc0-docx/014",  # pinned by p10_media
    "cc0-docx/068",  # pinned by p10_media (empty font6.odttf)
    "cc0-docx-1/066",  # tblHeader, bwMode, AlternateContent, RTL
    "cc0/073",  # themeColor, themeFontLang eastAsia
    "cc0-docx/005",  # AlternateContent
    "cc0-docx/020",  # ins/del, chart
    "cc0-docx/028",  # m:oMath
    "cc0-docx/100",  # bwMode=auto, blip cstate=print (p11_graphics)
    "cc0/099",  # bwMode, PNG, WMF
    "cc0/041",  # comments, Cyrillic
    "cc0/025",  # endnotes
    "cc0-docx/013",  # OLE, WMF
    "cc0/023",  # themeFill, CJK
    "cc0/081",  # comments, character styles
    "cc0/035",  # customXml, CJK
    "cc0-docx/090",  # endnotes
    "cc0-docx-1/010",  # OLE, math
    "cc0/075",  # Cyrillic
    "cc0-docx-1/065",  # comments
    "cc0-docx/058",  # tblHeader
    "cc0/067",  # the only SmartArt
    "cc0-docx/067",  # glossary
    "cc0/020",  # wpg:wgp (former AUD-103)
    "cc0-docx-1/024",  # embedded fonts
    "cc0-docx/077",  # second chart
    "cc0-docx/085",  # revisions, CJK
)

# Markup features: regular expressions over the XML parts under word/.
MARKUP = {
    "alternate_content": r"<mc:AlternateContent[\s>]",
    "bidi": r"<w:bidi[\s/>]",
    "blip_cstate": r"<a:blip\b[^>]*\bcstate=",
    "bw_mode": r"\bbwMode=",
    "character_styles": r"<w:style\b[^>]*w:type=\"character\"",
    "cols": r"<w:cols[\s/>]",
    "comments": r"<w:commentReference[\s/>]",
    "del": r"<w:del[\s>]",
    "drawing": r"<w:drawing[\s>]",
    "endnotes": r"<w:endnoteReference[\s/>]",
    "fld_char": r"<w:fldChar[\s/>]",
    "footnotes": r"<w:footnoteReference[\s/>]",
    "grid_span": r"<w:gridSpan[\s/>]",
    "header_ref": r"<w:headerReference[\s/>]",
    "ins": r"<w:ins[\s>]",
    "omath": r"<m:oMath[\s>]",
    "ole": r"<o:OLEObject[\s/>]",
    "pct_pos_v_offset": r"pctPosVOffset",
    "rtl": r"<w:rtl[\s/>]",
    "sdt": r"<w:sdt[\s>]",
    "tbl": r"<w:tbl[\s>]",
    "tbl_header": r"<w:tblHeader[\s/>]",
    "theme_color": r"\bw:themeColor=",
    "theme_fill": r"\bw:themeFill=",
    "theme_font_lang_east_asia": r"<w:themeFontLang\b[^>]*\bw:eastAsia=",
    "v_merge": r"<w:vMerge[\s/>]",
    "vml_pict": r"<w:pict[\s>]",
    "vml_textbox": r"<v:textbox[\s>]",
    "wp_anchor": r"<wp:anchor[\s>]",
    "wpg_group": r"<wpg:wgp[\s>]",
}

# Package features: part-name predicates.
PARTS = {
    "charts": lambda name: name.startswith("word/charts/"),
    "custom_xml": lambda name: name.startswith("customXml/"),
    "embedded_fonts": lambda name: name.startswith("word/fonts/"),
    "glossary": lambda name: name.startswith("word/glossary/"),
    "png": lambda name: name.lower().endswith(".png"),
    "smartart": lambda name: name.startswith("word/diagrams/"),
    "wmf_emf": lambda name: name.lower().endswith((".wmf", ".emf")),
}

# Writing systems, from the characters of word/document.xml (the manifests'
# `language` field is unreliable, plan §2.1).
SCRIPTS = {
    "script_cjk": r"[぀-ヿ㐀-䶿一-鿿가-힯]",
    "script_cyrillic": r"[Ѐ-ӿ]",
    "script_indic": r"[ऀ-෿]",
    "script_rtl": r"[֐-ࣿ]",
}


def sha256_of(path: str) -> str:
    digest = hashlib.sha256()
    with open(path, "rb") as handle:
        for chunk in iter(lambda: handle.read(1 << 20), b""):
            digest.update(chunk)
    return digest.hexdigest()


def scan_features(path: str) -> list[str]:
    found = set()
    with zipfile.ZipFile(path) as package:
        names = package.namelist()
        for feature, predicate in PARTS.items():
            if any(predicate(name) for name in names):
                found.add(feature)
        markup = []
        for name in names:
            if name.startswith("word/") and name.endswith(".xml"):
                markup.append(package.read(name).decode("utf-8", errors="replace"))
        text = "\n".join(markup)
        for feature, pattern in MARKUP.items():
            if re.search(pattern, text):
                found.add(feature)
        if "word/document.xml" in names:
            body = package.read("word/document.xml").decode("utf-8", errors="replace")
            for feature, pattern in SCRIPTS.items():
                if re.search(pattern, body):
                    found.add(feature)
    return sorted(found)


def collect(root: str) -> list[dict]:
    docs = []
    for directory, prefix, field in CORPORA:
        manifest_path = os.path.join(root, directory, "manifest.json")
        with open(manifest_path, encoding="utf-8") as handle:
            manifest = json.load(handle)
        for entry in manifest:
            name = entry[field]
            number = name.split("_", 1)[0]
            if not (len(number) == 3 and number.isdigit()):
                raise SystemExit(f"error: {directory}/{name}: no NNN_ prefix for a stable id")
            if entry["license_url"] not in LICENSE_URLS:
                raise SystemExit(f"error: {directory}/{name}: licence {entry['license_url']}")
            url = entry["download_url"]
            if not url.startswith("https://archive.org/"):
                raise SystemExit(f"error: {directory}/{name}: url {url} is not archive.org")
            path = os.path.join(root, directory, name)
            if not os.path.isfile(path):
                raise SystemExit(f"error: {path} is missing; the feature scan needs every document")
            size = os.path.getsize(path)
            digest = sha256_of(path)
            if digest != entry["sha256"] or size != entry["byte_size"]:
                raise SystemExit(f"error: {path} does not match its manifest (sha256/size)")
            docs.append(
                {
                    "id": f"{prefix}/{number}",
                    "path": f"{directory}/{name}",
                    "sha256": digest,
                    "bytes": size,
                    "url": url,
                    "license": "CC0-1.0",
                    "source_manifest": f"{directory}/manifest.json",
                    "features": scan_features(path),
                }
            )
    ids = [doc["id"] for doc in docs]
    if len(set(ids)) != len(ids):
        raise SystemExit("error: duplicate ids")
    unknown = set(CI_CORE) - set(ids)
    if unknown:
        raise SystemExit(f"error: ci-core names unknown ids: {sorted(unknown)}")
    for doc in docs:
        doc["tiers"] = ["ci-core", "ci-full"] if doc["id"] in CI_CORE else ["ci-full"]
    docs.sort(key=lambda doc: doc["id"])
    return docs


def quote(value: str) -> str:
    # A JSON string with non-ASCII kept is a valid TOML basic string.
    return json.dumps(value, ensure_ascii=False)


def render(docs: list[dict]) -> str:
    core = [doc for doc in docs if "ci-core" in doc["tiers"]]
    lines = [
        "# CC0 corpus lock (docs/CC0_CORPUS_MIGRATION_PLAN.md §4.1).",
        "#",
        "# GENERATED by `python3 xtool/corpus/make_lock.py --root <testdata>`; do not edit.",
        "# Tier membership lives in that script (`CI_CORE`). The documents themselves",
        "# are never committed: `cargo run -p xtool -- corpus fetch --tier ci-core`",
        "# downloads them into `testdata/` and checks every hash.",
        "#",
        f"# {len(docs)} documents, {sum(d['bytes'] for d in docs)} bytes; "
        f"ci-core: {len(core)} documents, {sum(d['bytes'] for d in core)} bytes.",
        "",
        "schema = 1",
    ]
    for doc in docs:
        lines.append("")
        lines.append("[[doc]]")
        lines.append(f"id = {quote(doc['id'])}")
        lines.append(f"path = {quote(doc['path'])}")
        lines.append(f"sha256 = {quote(doc['sha256'])}")
        lines.append(f"bytes = {doc['bytes']}")
        lines.append(f"url = {quote(doc['url'])}")
        lines.append(f"license = {quote(doc['license'])}")
        lines.append(f"source_manifest = {quote(doc['source_manifest'])}")
        lines.append("tiers = [" + ", ".join(quote(t) for t in doc["tiers"]) + "]")
        lines.append("features = [" + ", ".join(quote(f) for f in doc["features"]) + "]")
    return "\n".join(lines) + "\n"


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--root", default=os.path.join(REPO, "testdata"))
    parser.add_argument("--out", default=DEFAULT_OUT)
    parser.add_argument("--check", action="store_true", help="fail if --out differs")
    args = parser.parse_args(argv)
    text = render(collect(args.root))
    if args.check:
        try:
            with open(args.out, encoding="utf-8") as handle:
                current = handle.read()
        except FileNotFoundError:
            current = ""
        if current != text:
            print(f"error: {args.out} is stale; rerun make_lock.py", file=sys.stderr)
            return 1
        print(f"ok: {args.out} is current")
        return 0
    os.makedirs(os.path.dirname(os.path.abspath(args.out)), exist_ok=True)
    with open(args.out, "w", encoding="utf-8", newline="\n") as handle:
        handle.write(text)
    print(f"wrote {args.out}")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
