#!/usr/bin/env python3
"""The census gate: what the Transitional -> Strict path produces, measured.

`docs/transitional-to-strict-audit.md` §13 item 13. The audit that filled the
normalizer's queue ran on a one-off harness outside the tree, and every number in
that audit was therefore a number nobody could reproduce:

> "The census harness is not vendored: that is a queue item, see §9. It is needed
> because 'a tool that quietly does not measure what it judges' is exactly the
> category of error the gate has already closed four times
> (`docs/stage-10-xsd-audit.md` §3)."

This is that harness. It reuses `xsd_gate.Oracle` unchanged - the same patched
schema set, the same drivers, the same MCE processing, the same three baskets -
because a second oracle is a second definition of "Strict", and a threshold taken
from one and applied to the other is a number about nothing (the reason
`strict-ooxml-fidelity` exists, for the pixel gates).

What it measures
----------------
For each Transitional document: the package our own writer produces with
`write --transitional`, validated against the official Strict schemas. Two
numbers per document and the delta, exactly as `xsd_gate.py` prints for the
Strict corpus, plus:

* **census families** (`census.toml`, `TZ-nn`) - the constructs the audit found
  and what they cost. An item closes on a **measured zero**, never on a code
  review, and `origin` decides what a non-zero count means: `ours` fails the
  gate, `source` is markup the producer shipped and we carry verbatim.
* **loss report** - the normalizer's own accounting, because the audit's first
  finding was that it lied: 56 of 58 documents were marked `lossy` on nothing but
  `w:compat`. A gate that only watched the schemas could not see that.

Usage
-----
    python xtool/xsd-gate/census_gate.py                     # the whole census
    python xtool/xsd-gate/census_gate.py --write-reports DIR  # keep the loss reports
    python xtool/xsd-gate/census_gate.py --no-build

Exit codes: 0 clean, 1 census items of ours open / unmatched schema / real
unnamed loss / unclassified inventory, 2 the harness could not measure.

R03 classification (2026-10-05)
-------------------------------
Inventory element changes and XSD messages are different measurements:

* `unmatched_schema` — a libxml2 schema message matching no registry item. FAIL.
* `unclassified_element_changes` — a Strict-declared element that vanished from a
  still-present part and matches no disposition item. Leaves acceptance
  incomplete; it is **not** a schema error.
* named losses / declared transforms — `signal = "element"` registry items with
  a non-`ours` origin (usually `waived`). Counted and OK when non-zero.
* real unnamed losses — `ours` hits on `element` / `unaccounted` (and other
  owned signals). FAIL.
"""

from __future__ import annotations

import argparse
import collections
import fnmatch
import hashlib
import json
import math
import os
import posixpath
import re
import shutil
import subprocess
import sys
import tempfile
import time
import tomllib
import urllib.parse
import zipfile

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

try:
    from lxml import etree  # noqa: F401  (imported for the version banner only)
except ImportError:  # pragma: no cover - the gate's own dependency
    sys.stderr.write(
        "error: the census gate needs lxml: `pip install -r "
        "xtool/xsd-gate/requirements.txt`\n"
    )
    raise SystemExit(2)

# The markup-compatibility namespace. Defined here rather than borrowed from
# `xsd_gate` because the two tools use it for opposite purposes: there it selects
# which driver validates a part, here it says which branches of the input are
# alternatives of one another rather than two separate constructs.
MC_URI = "http://schemas.openxmlformats.org/markup-compatibility/2006"
MC = "{" + MC_URI + "}"

import xsd_gate  # noqa: E402

HERE = os.path.dirname(os.path.abspath(__file__))
REPO = xsd_gate.REPO
VERSION = "census-gate 1.1.0"

# Preferred prefixes for inventory matching. Transitional and Strict URIs of the
# same vocabulary share a prefix so a registry item can say `w:left` once.
NS_PREFIX = {
    "http://schemas.openxmlformats.org/wordprocessingml/2006/main": "w",
    "http://purl.oclc.org/ooxml/wordprocessingml/main": "w",
    "http://schemas.openxmlformats.org/drawingml/2006/main": "a",
    "http://purl.oclc.org/ooxml/drawingml/main": "a",
    "http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing": "wp",
    "http://purl.oclc.org/ooxml/drawingml/wordprocessingDrawing": "wp",
    "http://schemas.openxmlformats.org/officeDocument/2006/extended-properties": "ep",
    "http://purl.oclc.org/ooxml/officeDocument/extendedProperties": "ep",
    "http://schemas.openxmlformats.org/officeDocument/2006/docPropsVTypes": "vt",
    "http://purl.oclc.org/ooxml/officeDocument/docPropsVTypes": "vt",
    "http://schemas.openxmlformats.org/officeDocument/2006/custom-properties": "cust",
    "http://purl.oclc.org/ooxml/officeDocument/customProperties": "cust",
    "http://schemas.openxmlformats.org/drawingml/2006/chart": "c",
    "http://purl.oclc.org/ooxml/drawingml/chart": "c",
    "http://schemas.openxmlformats.org/officeDocument/2006/math": "m",
    "http://purl.oclc.org/ooxml/officeDocument/math": "m",
    "http://schemas.openxmlformats.org/drawingml/2006/picture": "pic",
    "http://purl.oclc.org/ooxml/drawingml/picture": "pic",
    "http://schemas.openxmlformats.org/drawingml/2006/spreadsheetDrawing": "xdr",
    "http://schemas.openxmlformats.org/package/2006/metadata/core-properties": "cp",
    "http://purl.oclc.org/ooxml/officeDocument/relationships": "r",
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships": "r",
}

# The corpus is Russian and Polish, and the console on this platform is cp1251.
# A gate that dies printing a document name is a gate that cannot be run, which
# is worse than a gate that is red: the numbers it would have published are the
# ones every queue item closes on. Replacement, never an exception - the report
# is about schema violations, and a name that renders as `?` does not change one.
for _stream in (sys.stdout, sys.stderr):
    try:
        _stream.reconfigure(encoding="utf-8", errors="replace")
    except (AttributeError, ValueError):  # pragma: no cover - non-reconfigurable stream
        pass

# Our own writer is the instrument under test here, so its output is decoded the
# way the writer means it. `text=True` would decode with the locale's charmap and
# raise UnicodeDecodeError deep inside a reader thread, which loses the message
# and the exit code together.
WRITER_IO = {"capture_output": True, "encoding": "utf-8", "errors": "replace"}

EXIT_OK = xsd_gate.EXIT_OK
EXIT_OPEN = xsd_gate.EXIT_VIOLATIONS
EXIT_UNMEASURABLE = xsd_gate.EXIT_UNMEASURABLE

# The Transitional corpus. Two directories, both committed, both the ones the
# audit measured: our own parser fixtures and the public sample documents. One
# number that mixes them would be two numbers wearing one name, so each is
# reported separately and the gate fails on their sum.
CORPORA = {
    "docx": os.path.join(REPO, "strict-ooxml-core", "tests", "docx"),
    "samples": os.path.join(REPO, "strict-ooxml-core", "tests", "samples"),
    "cc0": os.path.join(REPO, "testdata", "CC0_DOCX"),
    # The other two CC0 sets of testdata-lock/cc0.toml (fetched by
    # `xtool corpus fetch --tier ci-full`); off unless --corpora names them.
    "cc0-a": os.path.join(REPO, "testdata", "CC0"),
    "cc0-1": os.path.join(REPO, "testdata", "CC0_DOCX_1"),
}
DEFAULT_CORPORA = ("docx", "samples", "cc0")

# The counters a baseline holds. A ratchet fails when any of them grows; the
# census is not yet at zero (D05), so "no worse than recorded" is the gate CI
# can enforce today.
BASELINE_KEYS = ("missing", "unmatched_schema", "unclassified_element_changes", "ours")


def load_census() -> list[dict]:
    path = os.path.join(HERE, "census.toml")
    if not os.path.exists(path):
        raise SystemExit(f"error: {path} is missing; a census with no registry is a count")
    with open(path, "rb") as handle:
        return tomllib.load(handle)["item"]


def vanished_elements(
    source: str, written: str, oracle, named: set[str]
) -> list[tuple[str, str, str]]:
    """Strict-declared elements the input had and the written part does not.

    This is the `element` signal, and it is the only one of the five that can see
    the defect class the re-audit named as П-2, П-3 and П-9: a legal element that
    a REGENERATED part drops, silently. `TZ-15` covers parts and nothing else -
    `settings.xml` is present in both packages and still lost 49 of its elements,
    which no part-level signal can notice because a present part is not a lost
    part.

    Identity includes canonical namespaces of the element and parent. Known
    Transitional and Strict namespaces map to the same prefix; unrelated
    namespaces cannot cancel each other. Each finding carries a qualified label plus parent context in
    `detail`, because `w:left` under `w:tblBorders` (a declared T3 rename) is not
    the same change as an unexpected `w:left` elsewhere.

    Returns `(part, label, detail)` rows. `label` is `prefix:local` when the
    namespace is known, otherwise the bare local name. `detail` encodes
    `parent=<local>` for registry matching.
    """
    found: list[tuple[str, str, str]] = []
    with zipfile.ZipFile(source) as before, zipfile.ZipFile(written) as after:
        for part in sorted(before.namelist()):
            if not part.endswith(".xml") or part not in after.namelist():
                continue
            try:
                old = etree.fromstring(before.read(part))
                new = etree.fromstring(after.read(part))
            except etree.XMLSyntaxError:
                continue
            _drop_duplicate_singletons(old)
            _drop_duplicate_singletons(new)
            # `CT_RPr` allows one `w:rFonts`. A second sibling overrides only the
            # attributes it sets; the comparison uses that same overlay so a
            # repeated value is not a second fact and a slot the writer dropped
            # is still a change.
            _collapse_duplicate_rfonts(old)
            _collapse_duplicate_rfonts(new)
            if part == "word/styles.xml":
                _keep_last_duplicate_style(old)
                _keep_last_duplicate_style(new)
            old_rows = [
                (local, parent, namespace, parent_namespace)
                for local, parent, namespace, parent_namespace in _element_contexts(old)
                if local not in ("AlternateContent", "Choice", "Fallback")
            ]
            new_rows = [
                (local, parent, namespace, parent_namespace)
                for local, parent, namespace, parent_namespace in _element_contexts(new)
                if local not in ("AlternateContent", "Choice", "Fallback")
            ]
            old_counts = collections.Counter((local, parent, _prefix_or_uri(ns), _prefix_or_uri(pns)) for local, parent, ns, pns in old_rows)
            new_counts = collections.Counter((local, parent, _prefix_or_uri(ns), _prefix_or_uri(pns)) for local, parent, ns, pns in new_rows)
            namespace_of = {}
            for local, parent, namespace, parent_namespace in old_rows:
                namespace_of.setdefault((local, parent, _prefix_or_uri(namespace), _prefix_or_uri(parent_namespace)), namespace)
            for key in sorted(old_counts, key=lambda k: tuple(value or "" for value in k)):
                local, parent, _ns, _parent_ns = key
                removed = old_counts[key] - new_counts[key]
                if removed <= 0:
                    continue
                if local not in oracle.declared:
                    continue
                namespace = namespace_of[key]
                label = _qualified(local, namespace)
                # A report that names this element is evidence for a named loss.
                # It is not a reason to hide the row: a registry item titled
                # named_loss still has to see `named=1` for this input.
                named_flag = 1 if _is_named(named, local, label) else 0
                detail = (
                    f"parent={parent or ''}|namespace={_prefix_or_uri(namespace)}"
                    f"|parent_namespace={_parent_ns}"
                    f"|removed={removed}|named={named_flag}|{_cited_field(named)}"
                )
                found.append((part, label, detail))
            found.extend(
                _changed_attributes(old, new, oracle, named, part, new_counts, before, after)
            )
        found.extend(_changed_resources(before, after, named))
    return found


# Marks a writer is allowed to drop without the header becoming a different part.
# A paragraph, its text, and every other attribute stay in the digest.
_EDITOR_ATTRS = {
    "rsidR", "rsidRPr", "rsidDel", "rsidP", "rsidRDefault", "rsidTr",
    "rsidSect", "rsidTbl", "paraId", "textId", "anchorId", "editId",
}


def _strict_namespace_map() -> dict[str, str]:
    groups: dict[str, list[str]] = {}
    for uri, prefix in NS_PREFIX.items():
        groups.setdefault(prefix, []).append(uri)
    mapping: dict[str, str] = {}
    for uris in groups.values():
        strict = [uri for uri in uris if "purl.oclc.org/ooxml/" in uri]
        if len(strict) != 1:
            continue
        for uri in uris:
            mapping[uri] = strict[0]
    return mapping


_STRICT_NS = _strict_namespace_map()


def _semantic_part_digest(payload: bytes) -> str:
    """Header and footer identity after the Strict rewrite.

    Editor marks, `mc:Ignorable`, a default on/off `val`, and the complex-script
    font hint Strict rejects do not make a different part. Dropping a paragraph
    or changing its text does.
    """
    try:
        root = etree.fromstring(payload)
    except etree.XMLSyntaxError:
        return hashlib.sha256(payload).hexdigest()
    # Property bags are rewritten in schema order. Sorting them keeps a
    # reordered `w:rPr` from looking like a different header, and leaves
    # paragraph order alone so a dropped paragraph still changes the digest.
    property_bags = {"rPr", "pPr", "tcPr", "trPr", "tblPr", "sectPr", "pBdr", "rBdr"}

    def walk(element: etree._Element, chunks: list[str]) -> None:
        if not isinstance(element.tag, str):
            return
        qname = etree.QName(element)
        namespace = _STRICT_NS.get(qname.namespace or "", qname.namespace or "")
        chunks.append(f"<{namespace} {qname.localname}")
        attributes: list[str] = []
        for key, value in element.attrib.items():
            attr = etree.QName(key)
            if attr.localname in _EDITOR_ATTRS or attr.localname == "Ignorable":
                continue
            if qname.localname == "rFonts" and attr.localname == "hint" and value.lower() == "cs":
                continue
            if attr.localname == "val" and value.lower() in {"true", "on"}:
                continue
            attr_ns = _STRICT_NS.get(attr.namespace or "", attr.namespace or "")
            attributes.append(f"{attr_ns} {attr.localname}={value}")
        if attributes:
            chunks.append(" ".join(sorted(attributes)))
        text = (element.text or "").strip()
        if text:
            chunks.append(text)
        children = [child for child in element if isinstance(child.tag, str)]
        if qname.localname in property_bags:
            children.sort(key=lambda child: etree.QName(child).localname)
        for child in children:
            walk(child, chunks)

    chunks: list[str] = []
    walk(root, chunks)
    return hashlib.sha256("\n".join(chunks).encode("utf-8")).hexdigest()


def _load_rels(archive: zipfile.ZipFile, part: str) -> dict[str, tuple]:
    """Relationship identity survives id/part renaming, but not changed bytes.

    A raw Target comparison calls identical image bytes under img0.png a loss
    of image12.png. Resolve internal resources before comparing them. External
    targets stay exact, and relationship type/mode are part of the identity.
    """
    directory, _, file = part.rpartition("/")
    rels = f"{directory}/_rels/{file}.rels" if directory else f"_rels/{file}.rels"
    try:
        root = etree.fromstring(archive.read(rels))
    except (KeyError, etree.XMLSyntaxError):
        return {}
    found: dict[str, tuple] = {}
    for element in root:
        if not isinstance(element.tag, str):
            continue
        rid = element.get("Id")
        target = element.get("Target")
        if rid and target:
            kind = (element.get("Type") or "").replace(
                "http://schemas.openxmlformats.org/officeDocument/2006/relationships/",
                "http://purl.oclc.org/ooxml/officeDocument/relationships/",
            )
            mode = element.get("TargetMode") or "Internal"
            if mode == "External":
                found[rid] = (kind, mode, target)
                continue
            path, separator, fragment = target.partition("#")
            path = urllib.parse.unquote(path)
            resolved = posixpath.normpath(
                path.lstrip("/") if path.startswith("/")
                else posixpath.join(directory, path)
            )
            try:
                payload = archive.read(resolved)
            except KeyError:
                # An unresolved target is not evidence for equivalence.
                continue
            # A regenerated header or footer is a different zip entry even when
            # the paragraphs are the same. The relationship row is the part
            # identity; attribute losses inside the part stay their own rows.
            digest = (
                _semantic_part_digest(payload)
                if kind.endswith("/header") or kind.endswith("/footer")
                else hashlib.sha256(payload).hexdigest()
            )
            found[rid] = (kind, mode, digest, fragment if separator else "")
    return found


def _changed_attributes(
    old: etree._Element,
    new: etree._Element,
    oracle,
    named: set[str],
    part: str,
    new_elements: collections.Counter,
    before: zipfile.ZipFile,
    after: zipfile.ZipFile,
) -> list[tuple[str, str, str]]:
    """Attribute values that left a surviving element.

    A node that disappeared entirely is already an element row. This signal is
    the other half: the element is still there, and one of its properties or
    values is not. Identity is the attribute local name plus its value, so a
    Transitional-to-Strict namespace rewrite of the same value is not a change.
    """
    old_rels = _load_rels(before, part)
    new_rels = _load_rels(after, part)
    # Normalize before subtracting multisets: an old rId1 may have become
    # rId2 while the writer reused rId1 for a different resource. Cancelling
    # raw ids first loses both the equivalence and the target change.
    old_bag, namespace_of, old_stripped = _attribute_bag(old, old_rels)
    new_bag, _new_ns, new_stripped = _attribute_bag(new, new_rels)
    pane_left = _written_style_pane_sets(new_stripped)
    # `w:type="pct"` stores fiftieths (`5000`) in Transitional and `100%` in
    # Strict. Pair by element and type so a dxa `5000` cannot cancel a percent.
    _cancel_percent_widths(old_stripped, new_stripped, old_bag, new_bag)
    appeared = collections.Counter(
        {
            key: count
            for key, count in (new_bag - old_bag).items()
            if count > 0
        }
    )
    rows: list[tuple[str, str, str]] = []
    for key, old_count in sorted(old_bag.items(), key=lambda row: tuple(value or "" for value in row[0])):
        elem_local, parent, attr_local, value, elem_ns, parent_ns, attr_ns = key
        if elem_local not in oracle.declared:
            continue
        if new_elements[(elem_local, parent, elem_ns, parent_ns)] <= 0:
            continue
        removed = old_count - new_bag[key]
        if removed <= 0:
            continue
        # T3 renames left/right to start/end. T2 rewrites a twip count as the
        # same length in points. Either one, on the same element, is that
        # transform. A different length stays a change. Strict CT_Charset also
        # renames @val to @characterSet while keeping the same code-page value.
        candidates = [attr_local]
        renamed = {
            "left": "start",
            "right": "end",
            "leftChars": "startChars",
            "rightChars": "endChars",
        }.get(attr_local)
        if renamed is not None:
            candidates.append(renamed)
        if elem_local == "charset" and attr_local == "val":
            candidates.append("characterSet")
        for candidate in candidates:
            if removed <= 0:
                break
            for other_key, spare in list(appeared.items()):
                other_elem, other_parent, other_attr, other_value, other_ns, other_parent_ns, other_attr_ns = other_key
                same_namespace = (other_ns, other_parent_ns, other_attr_ns) == (elem_ns, parent_ns, attr_ns)
                # Word 2012 puts `tentative` in its own namespace. The Strict
                # attribute is `w:tentative`. A different value stays a change.
                if (
                    not same_namespace
                    and elem_local == "lvl"
                    and candidate == "tentative"
                    and _tentative_namespace(attr_ns)
                    and _tentative_namespace(other_attr_ns)
                ):
                    same_namespace = True
                if spare <= 0 or other_elem != elem_local or other_parent != parent or other_attr != candidate or not same_namespace:
                    continue
                same_value = _same_attr_value(
                    candidate, value, other_value, elem_local,
                    elem_ns,
                )
                if not same_value:
                    continue
                take = min(removed, spare)
                appeared[other_key] -= take
                removed -= take
        if (
            removed > 0
            and elem_local == "stylePaneFormatFilter"
            and attr_local == "val"
        ):
            expected = _style_pane_true_set(value)
            if expected is not None and pane_left[expected] > 0:
                take = min(removed, pane_left[expected])
                pane_left[expected] -= take
                removed -= take
        if removed <= 0:
            continue
        namespace = namespace_of.get((elem_local, parent, elem_ns, parent_ns))
        label = f"{_qualified(elem_local, namespace)}@{attr_local}"
        named_flag = 1 if _is_named(named, attr_local, label, elem_local) else 0
        detail = (
            f"parent={parent or ''}|namespace={_prefix_or_uri(namespace)}"
            f"|parent_namespace={parent_ns}|attribute_namespace={attr_ns}"
            f"|attr={attr_local}|was={value}|removed={removed}|named={named_flag}"
            f"|{_cited_field(named)}"
        )
        rows.append((part, label, detail))
    return rows


def _collapse_duplicate_rfonts(root: etree._Element) -> None:
    """Overlay repeated `w:rFonts` children of one `w:rPr` onto the first.

    The later element's attributes replace the same attributes on the first.
    Attributes the later element does not carry stay. Extra `rFonts` elements
    are removed. A single `rFonts` is left untouched.
    """
    for parent in list(root.iter()):
        if not isinstance(parent.tag, str) or etree.QName(parent).localname != "rPr":
            continue
        fonts = [
            child
            for child in list(parent)
            if isinstance(child.tag, str) and etree.QName(child).localname == "rFonts"
        ]
        if len(fonts) < 2:
            continue
        first = fonts[0]
        for extra in fonts[1:]:
            for key, value in extra.attrib.items():
                first.set(key, value)
            parent.remove(extra)


def _attribute_bag(
    root: etree._Element,
    relationships: dict[str, tuple] | None = None,
) -> tuple[collections.Counter, dict[tuple[str, str | None, str, str], str | None], etree._Element]:
    """Multiset of attributes with canonical element/parent/attribute namespaces."""
    bag: collections.Counter = collections.Counter()
    namespace_of = {}
    copy = etree.fromstring(etree.tostring(root))
    _strip_mce(copy)
    for element in copy.iter():
        if not isinstance(element.tag, str):
            continue
        qname = etree.QName(element)
        parent = element.getparent()
        parent_local = (
            etree.QName(parent).localname
            if parent is not None and isinstance(parent.tag, str)
            else None
        )
        parent_ns = etree.QName(parent).namespace if parent is not None and isinstance(parent.tag, str) else None
        elem_key = (qname.localname, parent_local, _prefix_or_uri(qname.namespace), _prefix_or_uri(parent_ns))
        namespace_of.setdefault(elem_key, qname.namespace)
        for key, value in element.attrib.items():
            attr = etree.QName(key).localname if key.startswith("{") else key
            namespace = etree.QName(key).namespace if key.startswith("{") else None
            if (
                relationships is not None
                and NS_PREFIX.get(namespace or "") == "r"
                and attr in {"id", "embed", "link"}
                and value in relationships
            ):
                value = "relationship:" + json.dumps(relationships[value], ensure_ascii=True)
            bag[(qname.localname, parent_local, attr, value, elem_key[2], elem_key[3], _prefix_or_uri(namespace))] += 1
    return bag, namespace_of, copy


def _changed_resources(
    before: zipfile.ZipFile, after: zipfile.ZipFile, named: set[str]
) -> list[tuple[str, str, str]]:
    """Non-XML parts whose bytes the written package no longer has.

    A missing part name is the `dropped` signal. A hash that is still present
    under any name is the same resource: a rename, and a second copy of
    identical bytes, are not a loss. A zip directory entry (a name ending in
    `/`) is not a payload. A hash whose count falls to zero — recompressed
    bytes, or a part that was removed — is an inventory change a schema
    cannot see.
    """
    def hashes(archive: zipfile.ZipFile) -> tuple[collections.Counter, dict[str, str]]:
        bag: collections.Counter = collections.Counter()
        names: dict[str, str] = {}
        for name in archive.namelist():
            if name.endswith("/") or name.endswith((".xml", ".rels", ".vml")):
                continue
            digest = hashlib.sha256(archive.read(name)).hexdigest()
            bag[digest] += 1
            names.setdefault(digest, name)
        return bag, names

    old_bag, old_names = hashes(before)
    new_bag, _new_names = hashes(after)
    rows: list[tuple[str, str, str]] = []
    for digest, old_count in sorted(old_bag.items()):
        new_count = new_bag[digest]
        if new_count > 0:
            continue
        # A hash that is gone is a resource the written package no longer has.
        name = old_names[digest]
        label = f"resource:{normalize_part(name)}"
        file_name = name.rsplit("/", 1)[-1]
        named_flag = 1 if name in named or file_name in named or label in named else 0
        detail = f"parent=|namespace=|sha256={digest}|named={named_flag}|removed={old_count}"
        rows.append((name, label, detail))
    return rows


def _element_contexts(root: etree._Element) -> list[tuple[str, str | None, str | None, str | None]]:
    """Element and parent QNames for every element after MCE strip."""
    copy = etree.fromstring(etree.tostring(root))
    _strip_mce(copy)
    rows: list[tuple[str, str | None, str | None, str | None]] = []
    for element in copy.iter():
        if not isinstance(element.tag, str):
            continue
        qname = etree.QName(element)
        parent = element.getparent()
        parent_local = etree.QName(parent).localname if parent is not None and isinstance(parent.tag, str) else None
        parent_namespace = etree.QName(parent).namespace if parent is not None and isinstance(parent.tag, str) else None
        rows.append((qname.localname, parent_local, qname.namespace, parent_namespace))
    return rows


def _prefix_or_uri(namespace: str | None) -> str:
    if not namespace:
        return ""
    return NS_PREFIX.get(namespace) or namespace


def _qualified(local: str, namespace: str | None) -> str:
    prefix = NS_PREFIX.get(namespace or "")
    return f"{prefix}:{local}" if prefix else local


def _local_of(label: str) -> str:
    _, separator, local = label.partition(":")
    return local if separator else label


def _local_names(root: etree._Element) -> set[str]:
    """Every local name in a part, counting neither namespaces nor branches."""
    return {local for local, _parent, _ns, _parent_ns in _element_contexts(root)}


# Namespaces the writer understands for `mc:Choice/@Requires` (AUD-50 ProcessChoice).
# Keep in sync with `strict_ooxml_wml::SUPPORTED_MCE_NAMESPACES`.
SUPPORTED_MCE_NAMESPACES = {
    "http://schemas.microsoft.com/office/word/2010/wordprocessingShape",
    "http://schemas.microsoft.com/office/word/2010/wordprocessingGroup",
    "http://schemas.microsoft.com/office/word/2010/wordprocessingDrawing",
    "http://purl.oclc.org/ooxml/officeDocument/math",
}
KNOWN_MCE_PREFIX_URI = {
    "wps": "http://schemas.microsoft.com/office/word/2010/wordprocessingShape",
    "wpg": "http://schemas.microsoft.com/office/word/2010/wordprocessingGroup",
    "wp14": "http://schemas.microsoft.com/office/word/2010/wordprocessingDrawing",
    "m": "http://purl.oclc.org/ooxml/officeDocument/math",
}


def _strip_mce(root: etree._Element) -> None:
    """Resolve Markup Compatibility the way the writer does, then drop ignorables.

    Inventory compares a source part to a regenerated part. The writer resolves
    `mc:AlternateContent` with ProcessChoice before modelling; counting both
    Choice and Fallback children as source inventory invents losses for every
    document that carries a dual DrawingML/VML branch (SoftUni, 009, …).
    """
    _resolve_alternate_content(root)
    ignorable: set[str] = set()
    for element in root.iter():
        if not isinstance(element.tag, str):
            continue
        value = element.get(f"{MC}Ignorable")
        if value:
            for prefix in value.split():
                uri = element.nsmap.get(prefix)
                if uri:
                    ignorable.add(uri)
    if not ignorable:
        return
    for element in list(root.iter()):
        if not isinstance(element.tag, str):
            continue
        uri = etree.QName(element).namespace
        if uri in ignorable:
            parent = element.getparent()
            if parent is not None:
                for child in list(element):
                    element.addprevious(child)
                parent.remove(element)


def _resolve_alternate_content(root: etree._Element) -> None:
    """Replace every `mc:AlternateContent` with the ProcessChoice branch."""
    ac_tag = f"{MC}AlternateContent"
    while True:
        element = root.find(f".//{ac_tag}")
        if element is None:
            return
        chosen = _select_mce_branch(element)
        parent = element.getparent()
        if parent is None:
            return
        index = list(parent).index(element)
        replacement = list(chosen) if chosen is not None else []
        tail = element.tail
        for offset, child in enumerate(replacement):
            parent.insert(index + offset, child)
        if replacement and tail:
            replacement[-1].tail = (replacement[-1].tail or "") + tail
        parent.remove(element)


def _select_mce_branch(ac: etree._Element) -> etree._Element | None:
    """Return the Choice/Fallback element whose children should be kept."""
    xmlns = {prefix: uri for prefix, uri in ac.nsmap.items() if prefix}
    fallback = None
    for child in ac:
        if not isinstance(child.tag, str) or etree.QName(child).namespace != MC_URI:
            continue
        local = etree.QName(child).localname
        for prefix, uri in child.nsmap.items():
            if prefix:
                xmlns[prefix] = uri
        if local == "Choice":
            requires = child.get("Requires") or ""
            if _mce_requires_understood(requires, xmlns):
                return child
        elif local == "Fallback":
            fallback = child
    return fallback


def _mce_requires_understood(requires: str, xmlns: dict[str, str]) -> bool:
    prefixes = requires.split()
    if not prefixes:
        return False
    for prefix in prefixes:
        uri = xmlns.get(prefix) or KNOWN_MCE_PREFIX_URI.get(prefix)
        if uri not in SUPPORTED_MCE_NAMESPACES:
            return False
    return True


def census_hits(
    registry: list[dict], signals: dict[str, list[tuple[str, str, str]]]
) -> dict:
    """Maps each signal onto the `TZ-nn` items.

    Schema messages and inventory element changes are classified separately:

    `unmatched_schema` — `message` rows matching no item. These are real XSD
    violations nobody named. They fail the gate as schema errors.
    `unclassified_element_changes` — `element` rows matching no disposition
    item. They keep acceptance incomplete but are **not** schema violations.

    A blanket drop of every unknown inventory row is forbidden: unclassified
    rows stay visible and prevent a clean census pass until each group has a
    disposition (named loss, declared transform, or owned `ours` loss).
    """
    counts = {item["id"]: 0 for item in registry}
    unmatched_schema: list[tuple[str, str, str]] = []
    unclassified_element_changes: list[tuple[str, str, str]] = []
    for signal, rows in signals.items():
        for where, label, detail in rows:
            for item in registry:
                if item.get("signal", "message") != signal:
                    continue
                if signal == "message":
                    hit = _message_matches(item, label, detail)
                elif signal == "dropped":
                    hit = any(
                        fnmatch.fnmatch(label, pattern) for pattern in item["elements"]
                    )
                elif signal == "unaccounted":
                    hit = any(
                        fnmatch.fnmatch(label, pattern) for pattern in item["elements"]
                    )
                elif signal == "picture":
                    hit = label == item["elements"][0]
                elif signal == "mce":
                    hit = any(marker in where for marker in item["elements"])
                elif signal == "extension":
                    hit = any(marker in where for marker in item["elements"])
                elif signal == "element":
                    hit = element_item_matches(item, where, label, detail)
                else:
                    hit = any(marker in detail for marker in item["elements"])
                if hit:
                    counts[item["id"]] += 1
                    break
            else:
                if signal == "message":
                    unmatched_schema.append((where, label, detail))
                elif signal == "element":
                    unclassified_element_changes.append((where, label, detail))
    return {
        "counts": counts,
        "unmatched_schema": unmatched_schema,
        "unclassified_element_changes": unclassified_element_changes,
        # Backward-compatible alias: historical callers that only knew "unmatched"
        # meant schema messages. Inventory must not ride this name.
        "unmatched": unmatched_schema,
    }


def _twips(text: str) -> float | None:
    if text.endswith("pt"):
        try:
            return float(text[:-2]) * 20.0
        except ValueError:
            return None
    # Producers emit `1872.0000000000002` and `-180.0`. The model keeps whole
    # twips; a residual under half a hundredth of a twip is the same length.
    if re.fullmatch(r"-?\d+(?:\.\d+)?", text):
        try:
            return float(text)
        except ValueError:
            return None
    return None


_WIDTH_ELEMENTS = frozenset({
    "tblW", "tcW", "gridCol", "tblInd", "tblCellSpacing", "wBefore", "wAfter",
})


def _fiftieths_percent(text: str) -> float | None:
    """`5000` fiftieths and `100%` are one Strict percentage width."""
    if text.endswith("%"):
        return _percent_number(text)
    if not re.fullmatch(r"-?\d+(?:\.\d+)?", text):
        return None
    number = float(text)
    if abs(number - round(number)) > 1e-6:
        return None
    return round(number) / 50.0


def _same_fiftieths(left: str, right: str) -> bool:
    a, b = _fiftieths_percent(left), _fiftieths_percent(right)
    return a is not None and b is not None and abs(a - b) < 1e-6


def _cancel_percent_widths(
    old: etree._Element,
    new: etree._Element,
    old_bag: collections.Counter,
    new_bag: collections.Counter,
) -> None:
    """Drop pct-width spellings that are the same fiftieths-of-a-percent value.

    The general attribute loop does not see `w:type`. Matching `5000` to `100%`
    without the type would also hide a dxa width of 5000 twips.
    """

    def rows(root: etree._Element) -> list[tuple]:
        found = []
        for element in root.iter():
            if not isinstance(element.tag, str):
                continue
            qname = etree.QName(element)
            if qname.localname not in _WIDTH_ELEMENTS:
                continue
            parent = element.getparent()
            if parent is None or not isinstance(parent.tag, str):
                parent_local, parent_ns = None, ""
            else:
                parent_q = etree.QName(parent)
                parent_local = parent_q.localname
                parent_ns = _prefix_or_uri(parent_q.namespace)
            elem_ns = _prefix_or_uri(qname.namespace)
            width = None
            width_ns = ""
            kind = ""
            for key, value in element.attrib.items():
                if key.startswith("{"):
                    attr_q = etree.QName(key)
                    local, namespace = attr_q.localname, _prefix_or_uri(attr_q.namespace)
                else:
                    local, namespace = key, ""
                if local == "w" and width is None:
                    width, width_ns = value, namespace or elem_ns
                elif local == "type" and not kind:
                    kind = value
            if width is None:
                continue
            identity = (qname.localname, parent_local, elem_ns, parent_ns, kind)
            found.append((identity, width, width_ns))
        return found

    pools: dict[tuple, list] = {}
    for identity, width, width_ns in rows(new):
        pools.setdefault(identity, []).append([width, width_ns])
    for identity, width, width_ns in rows(old):
        if identity[4] != "pct":
            continue
        pool = pools.get(identity)
        if not pool:
            continue
        for slot in pool:
            other, other_ns = slot
            if other is None or _same_measure(width, other) or not _same_fiftieths(width, other):
                continue
            slot[0] = None
            _bag_drop(old_bag, identity, width, width_ns)
            _bag_drop(new_bag, identity, other, other_ns)
            break


def _bag_drop(bag: collections.Counter, identity: tuple, value: str, attr_ns: str) -> None:
    local, parent, elem_ns, parent_ns, _kind = identity
    key = (local, parent, "w", value, elem_ns, parent_ns, attr_ns)
    if bag[key] > 0:
        bag[key] -= 1


# Children a parent stores once. A second identical copy is producer noise;
# two grid columns of the same width are not in this set and both stay.
_SINGLETON_CHILDREN = {
    "rPr": {
        "sz", "szCs", "spacing", "w", "color", "kern", "position", "u",
        "rFonts", "b", "i", "bCs", "iCs", "highlight", "em", "vertAlign",
        "lang", "shd", "rStyle",
    },
    "pPr": {"spacing", "ind", "jc", "pStyle", "rPr", "pBdr", "shd", "tabs"},
}


def _drop_duplicate_singletons(root: etree._Element) -> None:
    for parent in root.iter():
        if not isinstance(parent.tag, str):
            continue
        allowed = _SINGLETON_CHILDREN.get(etree.QName(parent).localname)
        if not allowed:
            continue
        seen: set[tuple] = set()
        for child in list(parent):
            if not isinstance(child.tag, str):
                continue
            local = etree.QName(child).localname
            if local not in allowed or len(child):
                continue
            key = (local, tuple(sorted(child.attrib.items())))
            if key in seen:
                parent.remove(child)
            else:
                seen.add(key)


def _keep_last_duplicate_style(root: etree._Element) -> None:
    """A repeated `w:styleId` keeps the later definition. The model is a map."""
    if not isinstance(root.tag, str) or etree.QName(root).localname != "styles":
        return
    seen: dict[str, etree._Element] = {}
    for child in list(root):
        if not isinstance(child.tag, str) or etree.QName(child).localname != "style":
            continue
        style_id = next(
            (value for key, value in child.attrib.items() if etree.QName(key).localname == "styleId"),
            None,
        )
        if style_id is None:
            continue
        previous = seen.get(style_id)
        if previous is not None:
            root.remove(previous)
        seen[style_id] = child


def _round_half_away(number: float) -> int:
    """`f64::round`: halves go away from zero, matching the reader's twip store."""
    if number >= 0:
        return math.floor(number + 0.5)
    return math.ceil(number - 0.5)


def _same_measure(left: str, right: str) -> bool:
    if left == right:
        return True
    a, b = _twips(left), _twips(right)
    if a is None or b is None:
        return False
    if abs(a - b) < 0.051:
        return True
    # A fractional twip and the whole twip the reader stores are one length.
    # 3124 and 3125 differ by a whole twip and stay different.
    return (
        abs(a - b) < 1.0
        and (a == _round_half_away(a) or b == _round_half_away(b))
        and _round_half_away(a) == _round_half_away(b)
    )


def _same_hex(left: str, right: str) -> bool:
    """`44546A` and `44546a` are one DrawingML sRGB value."""
    if len(left) != len(right) or len(left) not in (3, 6, 8):
        return False
    return (
        all(c in "0123456789abcdefABCDEF" for c in left + right)
        and left.lower() == right.lower()
    )


def _percent_number(text: str) -> float | None:
    """`100` and `100%` are one zoom/percentage spelling after T2."""
    raw = text[:-1] if text.endswith("%") else text
    if not re.fullmatch(r"-?\d+(\.\d+)?", raw):
        return None
    try:
        return float(raw)
    except ValueError:
        return None


# Transitional `a:graphicData/@uri` values and the Strict URI the writer emits.
# A different vocabulary is not in this map and stays a loss.
_GRAPHIC_URI = {
    "http://schemas.openxmlformats.org/drawingml/2006/picture": "http://purl.oclc.org/ooxml/drawingml/picture",
    "http://schemas.openxmlformats.org/drawingml/2006/lockedCanvas": "http://purl.oclc.org/ooxml/drawingml/lockedCanvas",
    "http://schemas.openxmlformats.org/drawingml/2006/chart": "http://purl.oclc.org/ooxml/drawingml/chart",
    "http://schemas.openxmlformats.org/drawingml/2006/diagram": "http://purl.oclc.org/ooxml/drawingml/diagram",
}


# Transitional `w:stylePaneFormatFilter/@w:val` bits, in the writer's order.
# `0x0010` is reserved. A value that sets it is not this expansion.
_STYLE_PANE_BITS = (
    (0x0001, "allStyles"),
    (0x0002, "customStyles"),
    (0x0004, "latentStyles"),
    (0x0008, "stylesInUse"),
    (0x0020, "headingStyles"),
    (0x0040, "numberingStyles"),
    (0x0080, "tableStyles"),
    (0x0100, "directFormattingOnRuns"),
    (0x0200, "directFormattingOnParagraphs"),
    (0x0400, "directFormattingOnNumbering"),
    (0x0800, "directFormattingOnTables"),
    (0x1000, "clearFormatting"),
    (0x2000, "top3HeadingStyles"),
    (0x4000, "visibleStyles"),
    (0x8000, "alternateStyleNames"),
)


def _style_pane_true_set(value: str) -> frozenset[str] | None:
    """Boolean attributes the Strict writer emits for a legacy bitmask."""
    try:
        bits = int(value.strip(), 16)
    except ValueError:
        return None
    if bits & 0x0010:
        return None
    return frozenset(name for mask, name in _STYLE_PANE_BITS if bits & mask)


def _written_style_pane_sets(root: etree._Element) -> collections.Counter:
    bag: collections.Counter = collections.Counter()
    for element in root.iter():
        if not isinstance(element.tag, str):
            continue
        if etree.QName(element).localname != "stylePaneFormatFilter":
            continue
        names = set()
        for key, raw in element.attrib.items():
            local = etree.QName(key).localname if str(key).startswith("{") else key
            if raw.lower() in {"1", "true", "on"}:
                names.add(local)
        bag[frozenset(names)] += 1
    return bag


def _same_sym_char(left: str, right: str) -> bool:
    def code(value: str) -> int | None:
        try:
            return int(value, 16)
        except ValueError:
            return None

    def norm(value: int) -> int:
        if 0xF000 <= value <= 0xF0FF:
            return value - 0xF000
        return value

    old, new = code(left), code(right)
    if old is None or new is None:
        return False
    return norm(old) == norm(new)


def _same_graphic_uri(left: str, right: str) -> bool:
    def canon(value: str) -> str:
        return _GRAPHIC_URI.get(value, value)
    return canon(left) == canon(right)


def _tentative_namespace(namespace: str) -> bool:
    return namespace in {
        "w",
        "http://schemas.openxmlformats.org/wordprocessingml/2006/main",
        "http://purl.oclc.org/ooxml/wordprocessingml/main",
        "http://schemas.microsoft.com/office/word/2012/wordml",
    }


def _same_attr_value(
    attr: str, left: str, right: str,
    element: str | None = None, namespace: str | None = None,
) -> bool:
    """Lengths, T3/T4 direction words, and on/off spellings."""
    if element == "graphicData" and attr == "uri" and _same_graphic_uri(left, right):
        return True
    # AUD-45 remaps a Symbol-font private-use code F0xx onto the low byte.
    # A different character is not that remap.
    if element == "sym" and attr == "char" and _same_sym_char(left, right):
        return True
    if _same_measure(left, right):
        return True
    if _same_hex(left, right):
        return True
    # DrawingML ST_Percentage uses thousandths in Transitional and a percent
    # suffix in Strict. Angles/coordinates must never use this conversion.
    color_percent = {
        "tint", "shade", "alpha", "alphaOff", "alphaMod", "hueMod",
        "sat", "satOff", "satMod", "lum", "lumOff", "lumMod",
        "red", "redOff", "redMod", "green", "greenOff", "greenMod",
        "blue", "blueOff", "blueMod",
    }
    drawing_percent = namespace == "a" and (
        (attr == "val" and element in color_percent)
        or (element == "gs" and attr == "pos")
        or (element in {"fillToRect", "fillRect", "srcRect", "tileRect"} and attr in {"l", "t", "r", "b"})
        or (element in {"defRPr", "rPr", "endParaRPr"} and attr == "baseline")
        or (element in {"spcPct", "buSzPct"} and attr == "val")
        or (element == "miter" and attr == "lim")
    )
    if drawing_percent:
        a, b = _percent_number(left), _percent_number(right)
        if a is not None and b is not None:
            a = a if left.endswith("%") else a / 1000.0
            b = b if right.endswith("%") else b / 1000.0
            return abs(a - b) < 1e-9
    # Chart amounts are whole percents on both sides: Transitional `150`, Strict
    # `150%` (T4.chart-percent) - the same number, unlike DrawingML thousandths.
    chart_percent = namespace == "c" and attr == "val" and element in {
        "gapWidth", "gapDepth", "overlap", "lblOffset", "holeSize",
        "secondPieSize", "bubbleScale", "depthPercent", "hPercent",
    }
    if chart_percent:
        a, b = _percent_number(left), _percent_number(right)
        if a is not None and b is not None:
            return abs(a - b) < 1e-9
    # `w:w/@w:val` is ST_TextScale: Strict writes `90%`, Transitional writes `90`.
    # The number is a percentage of the normal character width, not a length.
    if namespace == "w" and element == "w" and attr == "val":
        a, b = _percent_number(left), _percent_number(right)
        if a is not None and b is not None:
            return abs(a - b) < 1e-6
    if attr == "percent":
        a, b = _percent_number(left), _percent_number(right)
        if a is not None and b is not None and abs(a - b) < 0.051:
            return True
    on_off = (
        {"1", "true", "on"},
        {"0", "false", "off"},
    )
    if any(left in group and right in group for group in on_off):
        return True
    if attr != "val":
        return False
    direction = (
        {"left", "start"},
        {"right", "end"},
    )
    return any(left in group and right in group for group in direction)


def _is_named(named: set[str], *candidates: str) -> bool:
    """Whether this inventory row's names appear in the write report tokens."""
    lowered = {token.lower() for token in named}
    return any(candidate.lower() in lowered for candidate in candidates if candidate)


def _cited_field(named: set[str]) -> str:
    """Qualified feature ids this input's report actually printed."""
    cited = sorted(token for token in named if ":" in token)
    return "cited=" + ",".join(cited)


def _element_pattern_matches(pattern: str, label: str) -> bool:
    """Qualified patterns match only that qualified name.

    `w:left` does not accept `a:left`. A bare local pattern accepts only an
    unqualified label. `*` and `a:*` are not dispositions: they used to hide
    every change in a regenerated part.
    """
    if not pattern or pattern == "*" or pattern.endswith(":*"):
        return False
    if ":" in pattern:
        return pattern == label
    return ":" not in label and pattern == label


def _message_matches(item: dict, label: str, detail: str) -> bool:
    locals_or_labels = set(item["elements"]) | {_local_of(x) for x in item["elements"]}
    if label not in locals_or_labels and _local_of(label) not in locals_or_labels:
        return False
    return any(pattern in detail for pattern in item.get("messages", []))


def element_item_matches(item: dict, where: str, label: str, detail: str) -> bool:
    """Whether an inventory row matches a `signal = "element"` census item.

    Matching uses the qualified label (`w:left`) and optional parent/part
    filters from the registry. A local-only match without those filters is
    still accepted when the item lists the bare local name, so older items keep
    working; new dispositions should set `parents` and/or `parts`.
    """
    patterns = item.get("elements") or []
    name_hit = any(_element_pattern_matches(pattern, label) for pattern in patterns)
    if not name_hit:
        return False
    # `named_loss` in the registry is not itself a loss report. The row is a
    # named loss only when this input's write report named the element.
    if item.get("disposition") == "named_loss" and "named=1" not in detail.split("|"):
        return False
    required = item.get("requires_cited") or []
    if required:
        cited = ""
        for piece in detail.split("|"):
            if piece.startswith("cited="):
                cited = piece[len("cited=") :]
                break
        cited_names = set(cited.split(",")) if cited else set()
        if not any(token in cited_names for token in required):
            return False

    was_values = item.get("was") or []
    if was_values:
        was = ""
        for piece in detail.split("|"):
            if piece.startswith("was="):
                was = piece[len("was=") :]
                break
        if was not in was_values:
            return False

    parents = item.get("parents") or []
    if parents:
        parent = ""
        for piece in detail.split("|"):
            if piece.startswith("parent="):
                parent = piece[len("parent=") :]
                break
        if parent not in parents:
            return False

    parts = item.get("parts") or []
    if parts:
        part = where.split(": ", 1)[-1] if ": " in where else where
        if not any(fnmatch.fnmatch(part, pattern) for pattern in parts):
            return False
    return True


def decide_census_gate(
    *,
    documents: int,
    validated: int,
    missing: int,
    unmatched_schema: int,
    unclassified_element_changes: int,
    our_total: int,
) -> tuple[int, str]:
    """Census exit decision with schema / inventory / owned-loss separation."""
    if documents <= 0 or validated <= 0:
        return (
            EXIT_UNMEASURABLE,
            f"FAIL: unmeasurable documents={documents} validated={validated} missing={missing}",
        )
    if missing:
        return (
            EXIT_UNMEASURABLE,
            f"FAIL: missing={missing} validated={validated}",
        )
    if unmatched_schema:
        return (
            EXIT_OPEN,
            f"FAIL: unmatched_schema={unmatched_schema} schema violation(s) match no registry item",
        )
    if our_total:
        return (
            EXIT_OPEN,
            f"FAIL: {our_total} owned census hit(s) (schema/loss/transform of ours) remain open",
        )
    if unclassified_element_changes:
        return (
            EXIT_OPEN,
            f"FAIL: unclassified_element_changes={unclassified_element_changes} "
            "(inventory incomplete; not a schema error)",
        )
    return (
        EXIT_OK,
        "PASS: schema-clean; inventory dispositions complete; no owned census hits open",
    )


def pictures_lost(source: str, written: str) -> list[str]:
    """Parts that carried a VML picture and whose output carries no picture at all.

    The acceptance measurement for queue item 10 (`v:shape#t75` → `wp:inline` /
    `wp:anchor` + `pic:pic`). It is stated as a *pair* — an image reference on the
    way in, none on the way out — and not as a count of `v:shape`, because:

    - a `v:shape` with no `v:imagedata` is not a picture, it is drawn geometry,
      and the audit's own `§12` says the VML shape language has no equivalent here;
    - a part can carry a picture the converter did not convert (a text box) and
      still carry *another* picture it did, so a per-document total is the only
      comparison that means anything.

    What it catches that no schema message can: a converted picture whose
    `a:blip/@r:embed` resolves to nothing, because a relationship that does not
    resolve is not a schema violation — it is a picture that draws at no size.
    """
    return [
        part
        for part in sorted(_image_references(source, b"v:imagedata"))
        if not _image_references(written, b"a:blip")
    ]


def _image_references(package: str, marker: bytes) -> set[str]:
    """The XML parts of `package` that use `marker`, with the count in each."""
    found: set[str] = set()
    with zipfile.ZipFile(package) as archive:
        for name in archive.namelist():
            if not name.endswith((".xml", ".vml")):
                continue
            if marker in archive.read(name):
                found.add(name)
    return found


def mce_blocks(package: str) -> int:
    """`mc:AlternateContent` blocks in a package's parts.

    Strict conformance is defined on the post-MCE part (ECMA-376 Part 1 §2.1
    clause (ii)), so a block that survived T6 is a conformance defect whatever
    the schema says — and no schema message exists for it, because
    `mce_process()` removes the markup *before* validation, which is exactly why
    this signal is needed at all.
    """
    needle = b"AlternateContent"
    total = 0
    with zipfile.ZipFile(package) as archive:
        for name in archive.namelist():
            if name.endswith(".xml") and needle in archive.read(name):
                total += archive.read(name).count(b"<mc:AlternateContent")
    return total
def part_ledger(source: str, written: str) -> collections.Counter:
    """Parts whose **bytes** the input carried and the output does not have.

    The measurement the audit 8 called the only way to see a silent loss: a
    part that is *absent* validates perfectly, so no schema message exists and
    only a part-by-part comparison finds it.

    **By digest, not by name**, and that is not a refinement - it is the
    difference between the question and a proxy for it. This writer renames the
    parts it carries: `word/media/image3.png` becomes `media/image1.png` and
    `word/fonts/Nunito-regular.ttf` becomes `fonts/font0.ttf`, because the new
    name comes from a stable enumeration rather than from the producer's. A name
    comparison therefore reports every renamed-but-carried part as dropped, and
    on `DOCX_13_Pages` that was all ten embedded fonts - a loss report full of
    parts that were in fact in the package, byte for byte. Comparing the SHA-256
    of each part's contents answers the real question: is there anything here we
    do not have?

    Two exclusions, each for a stated reason rather than for convenience:

    - a `.rels` whose **owner** is still in the output is a replacement, not a
      loss. `word/_rels/footnotes.xml.rels` describes the source's footnotes and
      this write wrote its own beside them; keeping the source's would leave the
      package declaring relationships the written part does not have. The test is
      the owner's presence, not a rels part's presence, because the writer is
      free to emit no `.rels` at all for a part that ended up with no
      relationships, and a check that required one would report that as a loss
      on every document in the corpus and make the signal useless;
    - a ZIP **directory entry** (`word/media/`) is not a part and has no bytes.
    """
    before, before_digests = _part_contents(source)
    after, after_digests = _part_contents(written)
    missing = {
        name
        for name in before - after
        if not (name.endswith(".rels") and cg_owner_is_present(name, after))
    }
    return collections.Counter(
        name for name in missing if _digest(source, name) not in after_digests
    )


def cg_owner_is_present(rels_name: str, parts: set[str]) -> bool:
    """Whether the part a `.rels` describes is still in the output.

    **The owner's presence, not a sibling `.rels`'** — and the first version got
    this wrong by collecting the owners of the rels *in the output* and comparing
    against those, which reported a loss on every document whose header or
    footnotes part has no relationships of its own. That is the majority of
    documents, and it made the signal worse than the name comparison it replaced.
    """
    owner = rels_owner(rels_name)
    return owner is not None and owner in parts


def _part_contents(package: str) -> tuple[set[str], set[bytes]]:
    """The normalized part names of `package` and the digests of their bytes."""
    names: set[str] = set()
    digests: set[bytes] = set()
    with zipfile.ZipFile(package) as archive:
        for name in archive.namelist():
            if name.endswith("/"):
                continue
            names.add(normalize_part(name))
            digests.add(hashlib.sha256(archive.read(name)).digest())
    return names, digests


def _digest(package: str, normalized: str) -> bytes:
    """The SHA-256 of the part `normalized` came from, or empty bytes if unknown."""
    with zipfile.ZipFile(package) as archive:
        for name in archive.namelist():
            if name.endswith("/"):
                continue
            if normalize_part(name) == normalized:
                return hashlib.sha256(archive.read(name)).digest()
    return b""


def rels_owner(rels_name: str) -> str | None:
    """`word/_rels/header1.xml.rels` -> `word/header1.xml`, or `None`."""
    directory, _, file = rels_name.rpartition("/")
    base = directory.removesuffix("/_rels")
    if base == directory:
        return None
    return f"{base}/{file.removesuffix('.rels')}"
DIGITS = re.compile(r"\d+")


def names_it(report: str, shape: str) -> bool:
    """Whether the write's own loss report names a dropped part.

    Three comparisons, because the report groups and the ledger does not: the
    write prints one line per **directory** (`word/_rels: 5 part(s) ... a.xml, b.xml`),
    while the ledger works in normalized part **shapes**
    (`word/media/image17.png` becomes `word/media/image#.png`). So a shape counts
    as named when the report contains the shape, the shape's directory, or the
    shape's own file name — the last because a report that says
    "`word/_rels/fontTable.xml.rels` (replaced by this write's own)" has named it
    even though `word/_rels` alone would be too coarse to tell it from a
    neighbour.
    """
    if shape in report:
        return True
    directory = shape.rsplit("/", 1)[0] if "/" in shape else shape
    if directory in report:
        return True
    file = shape.rsplit("/", 1)[-1]
    return file in report


def _names_in(report: str) -> set[str]:
    """Every element or part name the writer's own report mentions.

    The report prints one `[lossy]` or `[ignorable]` line per finding, each
    carrying the name that was removed. A named removal is a decision (ADR-0007),
    so the `element` signal needs the same vocabulary `names_it` builds for parts -
    and it needs it by ELEMENT name rather than by part shape, because the defect
    this signal exists for is an element leaving a part that is still present.
    """
    if not report:
        return set()
    found: set[str] = set()
    for token in re.findall(r"[A-Za-z_][A-Za-z0-9_.:@-]*", report):
        found.add(token)
        # `w:doNotWrapTextWithPunct` names the element both ways: the signal
        # compares LOCAL names, because the input and the output are in different
        # namespaces by construction. `str.lstrip("wmo")` would be the wrong fix
        # here - it strips a character SET, turning `mathPr` into `athPr`.
        _, separator, local = token.partition(":")
        if separator:
            found.add(local)
        if token.lower() == "ignorable":
            found.add("Ignorable")
    return found


def normalize_part(name: str) -> str:
    """`word/media/image17.png` -> `word/media/image#.png`."""
    return DIGITS.sub("#", name)


# Namespaces a Strict package must not carry. ECMA-376 Part 1 §2.1 clause (ii)
# defines conformance on the POST-MCE part, so the `xsd_gate` oracle files these in
# its `extension` basket and counts them as not-ours - which is the correct
# answer about a *document* and the wrong answer about a *writer*. We are not
# shipping a document for a consumer's MCE processor to clean up; we are claiming
# to have written a Strict part. So the census counts them directly, before MCE
# processing runs, because that is the thing the item is about (audit §3, XS-17's
# pass-through half: 3 `w14:paraId` in `word/comments.xml`).
PRODUCER_NAMESPACES = {
    "http://schemas.microsoft.com/office/word/2010/wordml": "w14",
    "http://schemas.microsoft.com/office/word/2012/wordml": "w15",
    "http://schemas.microsoft.com/office/word/2018/wordml": "w16",
    "http://schemas.microsoft.com/office/word/2010/wordprocessingDrawing": "wp14",
    "http://schemas.microsoft.com/office/word/2010/wordprocessingShape": "wps",
    "http://schemas.microsoft.com/office/word/2010/wordprocessingGroup": "wpg",
    "http://schemas.microsoft.com/office/drawing/2010/main": "a14",
    "http://schemas.microsoft.com/office/drawing/2014/main": "a16",
    "http://schemas.microsoft.com/office/drawing/2018/chart": "cx",
    "http://schemas.openxmlformats.org/markup-compatibility/2006": "mc",
}


def producer_markup(path: str) -> collections.Counter:
    """Producer extension nodes and attributes left in our written package.

    Every node in a namespace the ECMA set does not declare, keyed by its local
    name, counted **before** MCE processing: both the elements and the attributes,
    because the normalizer drops `w14` elements and used to carry `w14`
    attributes straight through (audit §3), and an attribute is the half a schema
    message never names.
    """
    found: collections.Counter = collections.Counter()
    with zipfile.ZipFile(path) as package:
        for name in sorted(package.namelist()):
            if name.endswith(".rels") or not name.endswith((".xml", ".vml")):
                continue
            try:
                root = etree.fromstring(package.read(name))
            except etree.XMLSyntaxError:
                continue
            for element in root.iter():
                if not isinstance(element.tag, str):
                    continue
                namespace = etree.QName(element).namespace
                if namespace in PRODUCER_NAMESPACES:
                    found[etree.QName(element).localname] += 1
                for key in element.attrib:
                    if not key.startswith("{"):
                        continue
                    attribute_ns = key[1:].split("}")[0]
                    if attribute_ns in PRODUCER_NAMESPACES:
                        found[key[1:].split("}")[1]] += 1
    return found



def document_names(corpus: str, only: set[str] | None = None) -> list[str]:
    """Sorted `.docx` basenames in `corpus`, optionally restricted by `--only`."""
    names = sorted(n for n in os.listdir(corpus) if n.endswith(".docx"))
    if only is None:
        return names
    return [name for name in names if name in only]


def write_transitional(
    corpus: str, destination: str, cli: str, only: set[str] | None = None
) -> tuple[int, list, dict]:
    """`strict-ooxml write --transitional` over every document in `corpus`.

    The exit code is not the question, and the reason is the same one
    `xsd_gate.write_corpus` gives: `write` exits 1 when the package was written
    but the report has losses, and it has already written the file by then. A
    package that dropped a construct is exactly the package this gate needs to
    look at. Only exit 2 - nothing written - counts as a refusal.

    The write's own report comes back too, and it is what the `unaccounted`
    signal is measured against: a part the write dropped **and did not name** is
    a loss it hid, and no schema message exists for one because a part that is
    absent validates perfectly.
    """
    os.makedirs(destination, exist_ok=True)
    written, refused = 0, []
    reports: dict[str, str] = {}
    for name in document_names(corpus, only):
        out = os.path.join(destination, name)
        if os.path.exists(out):
            for attempt in range(6):
                try:
                    os.remove(out)
                    break
                except PermissionError:
                    if attempt == 5:
                        raise
                    time.sleep(0.4)
        result = subprocess.run(
            [cli, "write", os.path.join(corpus, name), "--out", out, "--transitional"],
            **WRITER_IO,
        )
        reports[name] = (result.stdout or "") + "\n" + (result.stderr or "")
        if result.returncode == 2 or not os.path.exists(out):
            why = (result.stderr or result.stdout).strip().splitlines()[:1]
            refused.append((name, why[0] if why else ""))
            continue
        written += 1
    return written, refused, reports


def loss_report(
    corpus: str, cli: str, destination: str | None, only: set[str] | None = None
) -> tuple[list[dict], int]:
    """`strict-ooxml normalize` over every document: the normalizer's own account.

    Read as a measurement and not as a verdict: the audit's first finding was
    that this report marked 56 of 58 documents `lossy` while removing nothing
    that mattered, so a number here is only meaningful next to what it names.
    """
    rows: list[dict] = []
    failures = 0
    for name in document_names(corpus, only):
        result = subprocess.run(
            [cli, "normalize", os.path.join(corpus, name)],
            **WRITER_IO,
        )
        if result.returncode != 0:
            failures += 1
            rows.append({"document": name, "ok": False, "reason": result.stderr.strip()[:120]})
            continue
        text = result.stdout
        row = {"document": name, "ok": True, "raw": text}
        for key in ("lossy", "ignorable", "error", "warning"):
            marker = f"{key}:"
            for line in text.splitlines():
                stripped = line.strip()
                if stripped.startswith(marker):
                    try:
                        row[key] = int(stripped[len(marker):].strip())
                    except ValueError:
                        pass
        rows.append(row)
        if destination:
            os.makedirs(destination, exist_ok=True)
            with open(
                os.path.join(destination, name + ".loss.txt"), "w", encoding="utf-8", newline=""
            ) as handle:
                handle.write(text)
    return rows, failures


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(
        description="Census the Transitional -> Strict path against the Strict schemas."
    )
    parser.add_argument("--cli", help="path to the strict-ooxml binary")
    parser.add_argument("--no-build", action="store_true")
    parser.add_argument(
        "--write-reports",
        help="keep the normalizer's loss reports in this directory",
    )
    parser.add_argument("--quiet-messages", action="store_true")
    parser.add_argument("--inventory-out", help="save exact unclassified inventory rows as JSON")
    parser.add_argument(
        "--corpora",
        help="comma-separated corpus labels to measure (default: docx,samples,cc0; "
        "also cc0-a, cc0-1)",
    )
    parser.add_argument(
        "--baseline",
        help="ratchet: pass when no counter exceeds this JSON's (see --baseline-out)",
    )
    parser.add_argument("--baseline-out", help="write the measured counters as a baseline JSON")
    parser.add_argument(
        "--keep-written",
        help="write the Transitional corpus into this directory and keep it",
    )
    parser.add_argument(
        "--only",
        action="append",
        default=[],
        metavar="NAME.docx",
        help=(
            "measure only these document basenames (repeatable). "
            "Used for per-package witness slices; omit for the full 221."
        ),
    )
    # Judging a pre-built `--written` tree is refused: the `unaccounted` signal
    # needs the write's own loss report. `--keep-written` is different — this
    # process still performs the writes and keeps the bytes for receipts.
    parser.add_argument("--written", help=argparse.SUPPRESS)
    args = parser.parse_args(argv)
    if args.written:
        raise SystemExit(
            "error: census_gate.py measures the write's own loss report, so it cannot judge "
            "packages it did not write.\n"
            "       Use --keep-written DIR to retain packages this run produces, "
            "or xsd_gate.py --written for the Strict-input gate."
        )

    if args.corpora:
        wanted = [label.strip() for label in args.corpora.split(",") if label.strip()]
        unknown = [label for label in wanted if label not in CORPORA]
        if unknown:
            raise SystemExit(f"error: unknown corpus label(s): {', '.join(unknown)}")
    else:
        wanted = list(DEFAULT_CORPORA)
    for label in list(CORPORA):
        if label not in wanted:
            del CORPORA[label]

    config = xsd_gate.load_config()
    print("=" * 78)
    print(VERSION)
    print(f"schema:  {config['source']['name']} ({config['source']['edition']})")
    print(f"oracle:  lxml {etree.LXML_VERSION}, libxml2 {etree.LIBXML_VERSION}, python {sys.version.split()[0]}")
    for label, path in CORPORA.items():
        print(f"corpus:  {label:<8} {path}")

    directory = xsd_gate.locate_schemas(config)
    oracle = xsd_gate.Oracle(directory)
    # The same refusal as the gate it shares an oracle with: a schema that did not
    # compile takes its whole part set down with it, and a count that silently
    # omits those parts is a count of nothing.
    if oracle.failures:
        print(f"\nSCHEMAS THAT FAILED TO COMPILE ({len(oracle.failures)}):", file=sys.stderr)
        for failure in oracle.failures:
            print(f"  {failure}", file=sys.stderr)
        return EXIT_UNMEASURABLE
    print(f"schemas: {oracle.drivers} drivers compiled, {len(oracle.failures)} failures")
    if not xsd_gate.run_controls(oracle):
        print("\ncontrol: FAILED - the harness cannot tell valid from invalid", file=sys.stderr)
        return EXIT_UNMEASURABLE

    cli = xsd_gate.find_cli(args)

    temporary = None
    if args.keep_written:
        written_root = args.keep_written
        os.makedirs(written_root, exist_ok=True)
    else:
        written_root = tempfile.mkdtemp(prefix="strict-census-written-")
        temporary = written_root

    try:
        return report(args, oracle, cli, written_root)
    finally:
        if temporary:
            shutil.rmtree(temporary, ignore_errors=True)


def report(args, oracle: xsd_gate.Oracle, cli: str, written_root: str) -> int:
    registry = load_census()
    signals: dict[str, list[tuple[str, str, str]]] = collections.defaultdict(list)
    out_schema: collections.Counter = collections.Counter()
    dropped: collections.Counter = collections.Counter()
    unaccounted: collections.Counter = collections.Counter()
    silent_elements: collections.Counter = collections.Counter()
    silent_element_parts: list[str] = []
    lost_picture_parts: list[str] = []
    total_in = total_out = 0
    clean = 0
    documents = 0
    validated = 0
    missing = 0
    refused: list[tuple[str, str]] = []
    lossy_documents = 0
    lossy_total = 0
    lost_pictures = 0
    only = set(args.only) if args.only else None
    if only:
        print(f"slice:   {len(only)} document basename(s) via --only")

    for label, corpus in CORPORA.items():
        if not os.path.isdir(corpus):
            raise SystemExit(f"error: census corpus {corpus} is not there")
        selected = document_names(corpus, only)
        if only is not None and not selected:
            continue
        destination = os.path.join(written_root, label)
        # Always write here: the unaccounted/element signals need this process's
        # own loss report. Reusing a foreign tree would invent silent losses.
        count, problems, reports = write_transitional(corpus, destination, cli, only)
        for name, why in problems:
            refused.append((name, why))

        print(f"\n=== corpus `{label}`: {count} document(s), written with --transitional")
        print(f"  {'document':<44} {'IN':>5} {'OUT':>5} {'delta':>7} {'ext':>5} {'gone':>5}")
        label_in = label_out = 0
        label_clean = 0
        label_extension = 0
        label_dropped = 0
        for name in selected:
            documents += 1
            incoming = xsd_gate.validate_package(os.path.join(corpus, name), oracle)
            path = os.path.join(destination, name)
            if not os.path.exists(path):
                missing += 1
                refused.append((name, "our writer wrote nothing"))
                print(f"  {name:<42} {sum(incoming.schema.values()):>5} {'refused':>9}")
                label_in += sum(incoming.schema.values())
                continue
            validated += 1
            outgoing = xsd_gate.validate_package(path, oracle)
            incoming_count = sum(incoming.schema.values())
            outgoing_count = sum(outgoing.schema.values())
            label_in += incoming_count
            label_out += outgoing_count
            out_schema.update(outgoing.schema)
            signals["message"].extend(outgoing.messages)
            extensions = producer_markup(path)
            label_extension += sum(extensions.values())
            for element, count in extensions.items():
                signals["extension"].append((element, element, ""))
            gone = part_ledger(os.path.join(corpus, name), path)
            label_dropped += sum(gone.values())
            dropped.update(gone)
            for block in range(mce_blocks(path)):
                signals["mce"].append((f"{name}:{block}", "mc:AlternateContent", ""))
            for lost in pictures_lost(os.path.join(corpus, name), path):
                lost_pictures += 1
                lost_picture_parts.append(f"{name}: {lost}")
                signals["picture"].append((lost, "vml-picture-without-a-picture", ""))
            report = reports.get(name, "")
            for shape, count in gone.items():
                signals["dropped"].append((shape, shape, ""))
                if not names_it(report, shape):
                    unaccounted.update({shape: count})
                    signals["unaccounted"].append((name, shape, ""))
            for part, label, detail in vanished_elements(
                os.path.join(corpus, name), path, oracle, _names_in(report)
            ):
                signals["element"].append((f"{name}: {part}", label, detail))
                # `Counter.update({k: v})` ADDS v; it does not assign it, so the
                # first spelling of this doubled the count on every hit and printed
                # 9.2e19 findings over 232 elements.
                silent_elements[_local_of(label)] += 1
                silent_element_parts.append(f"{name}: {part} {label} {detail}")
            if outgoing_count == 0:
                label_clean += 1
                clean += 1
            flag = ""
            if outgoing_count and outgoing_count < incoming_count:
                flag = "  <-- improved, still open"
            print(
                f"  {name:<42} {incoming_count:>5} {outgoing_count:>5}"
                f" {outgoing_count - incoming_count:>+7}"
                f" {sum(extensions.values()):>5} {sum(gone.values()):>5}{flag}"
            )
        print(
            f"  {'TOTAL':<42} {label_in:>5} {label_out:>5} {label_out - label_in:>+7}"
            f" {label_extension:>5} {label_dropped:>5}   clean: {label_clean}/{count}"
        )
        total_in += label_in
        total_out += label_out

        rows, failures = loss_report(corpus, cli, args.write_reports, only)
        counts = collections.Counter()
        for row in rows:
            if not row["ok"]:
                failures += 1
                continue
            counts["lossy"] += row.get("lossy", 0)
            counts["ignorable"] += row.get("ignorable", 0)
            counts["error"] += row.get("error", 0)
            counts["warning"] += row.get("warning", 0)
            if row.get("lossy", 0) > 0:
                for line in row.get("raw", "").splitlines():
                    if "lossy" in line.lower() and ":" in line:
                        signals["lossy"].append((row["document"], "lossy", line))
        label_lossy_docs = sum(1 for row in rows if row.get("lossy", 0) > 0)
        lossy_documents += label_lossy_docs
        lossy_total += counts["lossy"]
        print(
            f"  loss report: {label_lossy_docs}/{len(rows)} document(s) marked lossy, "
            f"{counts['lossy']} lossy record(s), {counts['ignorable']} ignorable, "
            f"{counts['error']} error(s), {counts['warning']} warning(s)"
            + (f", {failures} normalize run(s) failed" if failures else "")
        )

    print(f"\n  {'TOTAL':<42} {total_in:>5} {total_out:>5} {total_out - total_in:>+7}"
          f" {sum(1 for _ in signals['extension']):>5} {sum(dropped.values()):>5}"
          f"   clean: {clean}/{documents}")
    if refused:
        print(f"packages our writer refused outright: {len(refused)}")
        for name, why in refused[:10]:
            print(f"  {name}: {why}")

    print(f"\n=== our output, by element: {sum(out_schema.values())} violation(s), "
          f"{len(out_schema)} distinct element(s)")
    for local, count in out_schema.most_common(60):
        print(f"  {local:<20} {count}")

    # The two signals no schema message can produce. A part that is absent
    # validates perfectly, and a node in a namespace the ECMA set does not declare
    # is removed by MCE before conformance is even defined - so both are losses
    # that a validator is structurally unable to see, which is why the audit §8
    # needed a measurement of its own.
    if signals["extension"]:
        print(f"\n=== producer extension nodes left in our output: {len(signals['extension'])}"
              " (elements AND attributes, before MCE - a schema message never names either)")
        by_node: collections.Counter = collections.Counter(
            element for element, _, _ in signals["extension"]
        )
        for element, count in by_node.most_common(20):
            print(f"  {element:<40} {count}")
    if dropped:
        print(f"\n=== parts the input carried and our output does not: {sum(dropped.values())}"
              " (a missing part validates perfectly, which is what makes this a silent loss)")
        for shape, count in dropped.most_common(30):
            flag = "" if shape not in unaccounted else "   <-- NOT in the write's report"
            print(f"  {shape:<50} {count}{flag}")
        if unaccounted:
            print(f"\n=== and of those, {sum(unaccounted.values())} the write did NOT name:"
                  " a loss nobody can see in the report")
            for shape, count in unaccounted.most_common(30):
                print(f"  {shape:<50} {count}")

    if lost_picture_parts:
        print(
            f"\n=== {lost_pictures} part(s) carried a VML picture and the written package has no "
            "picture at all:"
        )
        for part in sorted(set(lost_picture_parts)):
            print(f"  {part}")

    hits = census_hits(registry, signals)
    counts = hits["counts"]
    ours = {item["id"] for item in registry if item["origin"] == "ours"}
    print("\n=== census registry (`TZ-nn` closes on a measured zero, never on a code review)")
    for item in registry:
        count = counts[item["id"]]
        origin = item["origin"]
        signal = item.get("signal", "message")
        blind = item.get("blind_to")
        state = xsd_gate.item_state(item, count)
        # A zero from a signal that cannot see is not a zero. Printing `closed`
        # beside it without the reason is how `TZ-04` reported 0 for a rule that
        # had been dead all day, so the blindness is a column of its own.
        mark = f" [blind: {blind}]" if blind else ""
        print(f"  {item['id']:<7} {origin:<7} {signal:<9} {state:<12} {item['summary']}{mark}")
    if any(item.get("blind_to") for item in registry):
        print(
            "\n  A `blind` item is measured over a corpus that never exercises the rule, so its zero"
            "\n  is the absence of evidence. The gate for those is the unit test named in `audit`,"
            "\n  because a corpus gate cannot see what the corpus does not contain."
        )
    if silent_elements:
        # The inventory and the gate are different questions. Declared transforms
        # and named losses are matched by `signal = "element"` items; anything
        # left over is `unclassified_element_changes` (incomplete acceptance),
        # never an XSD schema failure by itself.
        print(
            f"\n=== Strict-declared elements a regenerated part dropped: "
            f"{sum(silent_elements.values())} finding(s) over {len(silent_elements)} distinct element(s)"
            "\n    The part is present in both packages, so TZ-15 sees nothing while its content"
            "\n    shrinks. Matched rows are named losses or declared transforms; unmatched rows"
            "\n    are unclassified_element_changes (not schema errors)."
        )
        for local, count in silent_elements.most_common(40):
            print(f"  {local:<28} {count:>4} document(s)")
    if hits["unmatched_schema"]:
        print(
            f"\n=== {len(hits['unmatched_schema'])} schema violation(s) match no census item"
        )
        for where, local, message in hits["unmatched_schema"][:40]:
            print(f"  {where} [{local}]: {message}")
    if hits["unclassified_element_changes"]:
        print(
            f"\n=== {len(hits['unclassified_element_changes'])} unclassified element change(s) "
            "(inventory incomplete; not a schema error)"
        )
        for where, local, detail in hits["unclassified_element_changes"][:40]:
            print(f"  {where} [{local}]: {detail}")

    if not args.quiet_messages and out_schema:
        print("\n=== every message, so nothing is counted on trust")
        for where, _, message in signals["message"]:
            print(f"  {where}: {message}")

    open_items = {item_id: counts[item_id] for item_id in sorted(ours) if counts[item_id]}
    our_total = sum(open_items.values())
    unmatched_schema_n = len(hits["unmatched_schema"])
    unclassified_n = len(hits["unclassified_element_changes"])
    if args.inventory_out:
        with open(args.inventory_out, "w", encoding="utf-8") as output:
            json.dump({
                "documents": documents, "validated": validated, "missing": missing,
                "unmatched_schema": unmatched_schema_n, "ours": our_total,
                "unclassified_element_changes": hits["unclassified_element_changes"],
            }, output, ensure_ascii=False, indent=2)
    print(
        f"\nmeasured: documents={documents} validated={validated} "
        f"missing={missing} unmatched_schema={unmatched_schema_n} "
        f"unclassified_element_changes={unclassified_n} ours={our_total}"
    )
    code, summary = decide_census_gate(
        documents=documents,
        validated=validated,
        missing=missing,
        unmatched_schema=unmatched_schema_n,
        unclassified_element_changes=unclassified_n,
        our_total=our_total,
    )
    measured = {
        "corpora": sorted(CORPORA),
        "documents": documents,
        "missing": missing,
        "unmatched_schema": unmatched_schema_n,
        "unclassified_element_changes": unclassified_n,
        "ours": our_total,
    }
    if args.baseline_out:
        with open(args.baseline_out, "w", encoding="utf-8") as output:
            json.dump(measured, output, indent=2, sort_keys=True)
            output.write("\n")
    if args.baseline:
        code, summary = ratchet(code, measured, args.baseline)
    print(f"\n{summary}")
    if open_items:
        print(
            "      open: "
            + ", ".join(f"{key}={value}" for key, value in open_items.items())
        )
    print(
        f"      ({documents - lossy_documents}/{documents} document(s) report no lossy record; "
        f"{lossy_total} lossy record(s) total - a removal the report names is not a defect, "
        "an un-named one would be)"
    )
    return code


def ratchet(code: int, measured: dict, baseline_path: str) -> tuple[int, str]:
    """The census against a recorded baseline: no counter may grow.

    An unmeasurable run stays unmeasurable. A baseline for other corpora is not
    a baseline for this run, so it is refused rather than compared.
    """
    if code == EXIT_UNMEASURABLE:
        return code, "ratchet: the census was not measurable"
    with open(baseline_path, encoding="utf-8") as handle:
        baseline = json.load(handle)
    if baseline.get("corpora") != measured["corpora"]:
        return EXIT_UNMEASURABLE, (
            f"ratchet: baseline corpora {baseline.get('corpora')} "
            f"!= measured {measured['corpora']}"
        )
    worse = [
        f"{key} {measured[key]} > {baseline.get(key, 0)}"
        for key in BASELINE_KEYS
        if measured[key] > baseline.get(key, 0)
    ]
    if worse:
        return EXIT_OPEN, "ratchet: FAILED - " + "; ".join(worse)
    better = [
        f"{key} {measured[key]} < {baseline.get(key, 0)}"
        for key in BASELINE_KEYS
        if measured[key] < baseline.get(key, 0)
    ]
    if better:
        return EXIT_OK, "ratchet: OK, improved (" + "; ".join(better) + ") - lower the baseline"
    return EXIT_OK, "ratchet: OK, no counter grew"


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
