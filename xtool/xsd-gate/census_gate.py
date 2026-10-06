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
import os
import re
import shutil
import subprocess
import sys
import tempfile
import time
import tomllib
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
MC = "{http://schemas.openxmlformats.org/markup-compatibility/2006}"

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
}


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

    Detection still compares LOCAL names across namespaces: the input is
    Transitional and the output is Strict, so a qualified comparison finds
    nothing. Each finding then carries a qualified label plus parent context in
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
            old_rows = [
                (local, parent, namespace)
                for local, parent, namespace in _element_contexts(old)
                if local not in ("AlternateContent", "Choice", "Fallback")
            ]
            new_rows = [
                (local, parent, namespace)
                for local, parent, namespace in _element_contexts(new)
                if local not in ("AlternateContent", "Choice", "Fallback")
            ]
            # Identity is local name plus parent, not the namespace URI.
            # Transitional and Strict spell the same WML element with different
            # URIs; counting those as a deletion hides nothing and reports
            # everything. Multiplicity still notices when one of two siblings
            # with the same local name and parent disappears.
            old_counts = collections.Counter((local, parent) for local, parent, _ns in old_rows)
            new_counts = collections.Counter((local, parent) for local, parent, _ns in new_rows)
            namespace_of: dict[tuple[str, str | None], str | None] = {}
            for local, parent, namespace in old_rows:
                namespace_of.setdefault((local, parent), namespace)
            for local, parent in sorted(old_counts):
                removed = old_counts[(local, parent)] - new_counts[(local, parent)]
                if removed <= 0:
                    continue
                if local not in oracle.declared:
                    continue
                namespace = namespace_of[(local, parent)]
                label = _qualified(local, namespace)
                # A report that names this element is evidence for a named loss.
                # It is not a reason to hide the row: a registry item titled
                # named_loss still has to see `named=1` for this input.
                named_flag = 1 if _is_named(named, local, label) else 0
                detail = (
                    f"parent={parent or ''}|namespace={_prefix_or_uri(namespace)}"
                    f"|removed={removed}|named={named_flag}|{_cited_field(named)}"
                )
                found.append((part, label, detail))
            found.extend(
                _changed_attributes(old, new, oracle, named, part, new_counts, before, after)
            )
        found.extend(_changed_resources(before, after, named))
    return found


def _load_rels(archive: zipfile.ZipFile, part: str) -> dict[str, str]:
    """`Id` → `Target` for the relationships of `part`."""
    directory, _, file = part.rpartition("/")
    rels = f"{directory}/_rels/{file}.rels" if directory else f"_rels/{file}.rels"
    try:
        root = etree.fromstring(archive.read(rels))
    except (KeyError, etree.XMLSyntaxError):
        return {}
    found: dict[str, str] = {}
    for element in root:
        if not isinstance(element.tag, str):
            continue
        rid = element.get("Id")
        target = element.get("Target")
        if rid and target:
            found[rid] = target
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
    old_bag, namespace_of = _attribute_bag(old)
    new_bag, _new_ns = _attribute_bag(new)
    old_rels = _load_rels(before, part)
    new_rels = _load_rels(after, part)
    appeared = collections.Counter(
        {
            key: count
            for key, count in (new_bag - old_bag).items()
            if count > 0
        }
    )
    rows: list[tuple[str, str, str]] = []
    for key, old_count in sorted(old_bag.items()):
        elem_local, parent, attr_local, value = key
        if elem_local not in oracle.declared:
            continue
        if new_elements[(elem_local, parent)] <= 0:
            continue
        removed = old_count - new_bag[key]
        if removed <= 0:
            continue
        # T3 renames left/right to start/end. T2 rewrites a twip count as the
        # same length in points. Either one, on the same element, is that
        # transform. A different length stays a change. Strict CT_Charset also
        # renames @val to @characterSet while keeping the same code-page value.
        candidates = [attr_local]
        renamed = {"left": "start", "right": "end"}.get(attr_local)
        if renamed is not None:
            candidates.append(renamed)
        if elem_local == "charset" and attr_local == "val":
            candidates.append("characterSet")
        for candidate in candidates:
            if removed <= 0:
                break
            for (other_elem, other_parent, other_attr, other_value), spare in list(appeared.items()):
                if spare <= 0 or other_elem != elem_local or other_parent != parent or other_attr != candidate:
                    continue
                same_value = _same_attr_value(candidate, value, other_value)
                same_target = (
                    attr_local in {"id", "embed"}
                    and old_rels.get(value)
                    and old_rels.get(value) == new_rels.get(other_value)
                )
                if not same_value and not same_target:
                    continue
                take = min(removed, spare)
                appeared[(other_elem, other_parent, other_attr, other_value)] -= take
                removed -= take
        if removed <= 0:
            continue
        namespace = namespace_of.get((elem_local, parent))
        label = f"{_qualified(elem_local, namespace)}@{attr_local}"
        named_flag = 1 if _is_named(named, attr_local, label, elem_local) else 0
        detail = (
            f"parent={parent or ''}|namespace={_prefix_or_uri(namespace)}"
            f"|attr={attr_local}|was={value}|removed={removed}|named={named_flag}"
            f"|{_cited_field(named)}"
        )
        rows.append((part, label, detail))
    return rows


def _attribute_bag(
    root: etree._Element,
) -> tuple[collections.Counter, dict[tuple[str, str | None], str | None]]:
    """Multiset of `(element, parent, attr, value)` plus element namespace."""
    bag: collections.Counter = collections.Counter()
    namespace_of: dict[tuple[str, str | None], str | None] = {}
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
        namespace_of.setdefault((qname.localname, parent_local), qname.namespace)
        for key, value in element.attrib.items():
            attr = etree.QName(key).localname if key.startswith("{") else key
            bag[(qname.localname, parent_local, attr, value)] += 1
    return bag, namespace_of


def _changed_resources(
    before: zipfile.ZipFile, after: zipfile.ZipFile, named: set[str]
) -> list[tuple[str, str, str]]:
    """Non-XML parts present on both sides whose bytes are not the same.

    A missing part is the `dropped` signal. A part that is still there but no
    longer the same resource is an inventory change a schema cannot see.
    """
    def hashes(archive: zipfile.ZipFile) -> tuple[collections.Counter, dict[str, str]]:
        bag: collections.Counter = collections.Counter()
        names: dict[str, str] = {}
        for name in archive.namelist():
            if name.endswith((".xml", ".rels", ".vml")):
                continue
            digest = hashlib.sha256(archive.read(name)).hexdigest()
            bag[digest] += 1
            names.setdefault(digest, name)
        return bag, names

    old_bag, old_names = hashes(before)
    new_bag, _new_names = hashes(after)
    rows: list[tuple[str, str, str]] = []
    for digest, old_count in sorted(old_bag.items()):
        removed = old_count - new_bag[digest]
        if removed <= 0:
            continue
        # Same bytes under a new part name are a rename, not a new resource.
        # A hash that is gone is a resource the written package no longer has.
        name = old_names[digest]
        label = f"resource:{normalize_part(name)}"
        file_name = name.rsplit("/", 1)[-1]
        named_flag = 1 if name in named or file_name in named or label in named else 0
        detail = f"parent=|namespace=|sha256={digest}|named={named_flag}|removed={removed}"
        rows.append((name, label, detail))
    return rows


def _element_contexts(root: etree._Element) -> list[tuple[str, str | None, str | None]]:
    """`(local, parent_local, namespace_uri)` for every element after MCE strip."""
    copy = etree.fromstring(etree.tostring(root))
    _strip_mce(copy)
    rows: list[tuple[str, str | None, str | None]] = []
    for element in copy.iter():
        if not isinstance(element.tag, str):
            continue
        qname = etree.QName(element)
        parent = element.getparent()
        parent_local = etree.QName(parent).localname if parent is not None and isinstance(parent.tag, str) else None
        rows.append((qname.localname, parent_local, qname.namespace))
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
    return {local for local, _parent, _ns in _element_contexts(root)}


def _strip_mce(root: etree._Element) -> None:
    """Removes every element in a namespace MCE declares ignorable."""
    ignorable: set[str] = set()
    for element in root.iter():
        if not isinstance(element.tag, str):
            continue
        value = element.get(f"{{{MC}}}Ignorable")
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
    if re.fullmatch(r"-?\d+", text):
        return float(text)
    return None


def _same_measure(left: str, right: str) -> bool:
    if left == right:
        return True
    a, b = _twips(left), _twips(right)
    return a is not None and b is not None and abs(a - b) < 0.051


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


def _same_attr_value(attr: str, left: str, right: str) -> bool:
    """Lengths, T3/T4 direction words, and on/off spellings."""
    if _same_measure(left, right):
        return True
    if _same_hex(left, right):
        return True
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
    for token in re.findall(r"[A-Za-z_][A-Za-z0-9_.:-]*", report):
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



def write_transitional(corpus: str, destination: str, cli: str) -> tuple[int, list, dict]:
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
    for name in sorted(os.listdir(corpus)):
        if not name.endswith(".docx"):
            continue
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


def loss_report(corpus: str, cli: str, destination: str | None) -> tuple[list[dict], int]:
    """`strict-ooxml normalize` over every document: the normalizer's own account.

    Read as a measurement and not as a verdict: the audit's first finding was
    that this report marked 56 of 58 documents `lossy` while removing nothing
    that mattered, so a number here is only meaningful next to what it names.
    """
    rows: list[dict] = []
    failures = 0
    for name in sorted(os.listdir(corpus)):
        if not name.endswith(".docx"):
            continue
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
    parser.add_argument(
        "--keep-written",
        help="write the Transitional corpus into this directory and keep it",
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

    for label, corpus in CORPORA.items():
        if not os.path.isdir(corpus):
            raise SystemExit(f"error: census corpus {corpus} is not there")
        destination = os.path.join(written_root, label)
        # Always write here: the unaccounted/element signals need this process's
        # own loss report. Reusing a foreign tree would invent silent losses.
        count, problems, reports = write_transitional(corpus, destination, cli)
        for name, why in problems:
            refused.append((name, why))

        print(f"\n=== corpus `{label}`: {count} document(s), written with --transitional")
        print(f"  {'document':<44} {'IN':>5} {'OUT':>5} {'delta':>7} {'ext':>5} {'gone':>5}")
        label_in = label_out = 0
        label_clean = 0
        label_extension = 0
        label_dropped = 0
        for name in sorted(n for n in os.listdir(corpus) if n.endswith(".docx")):
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

        rows, failures = loss_report(corpus, cli, args.write_reports)
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


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
