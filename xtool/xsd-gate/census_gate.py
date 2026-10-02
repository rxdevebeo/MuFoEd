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

Exit codes: 0 clean, 1 census items of ours open, 2 the harness could not measure.
"""

from __future__ import annotations

import argparse
import collections
import fnmatch
import os
import re
import shutil
import subprocess
import sys
import tempfile
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

import xsd_gate  # noqa: E402

HERE = os.path.dirname(os.path.abspath(__file__))
REPO = xsd_gate.REPO
VERSION = "census-gate 1.0.0"

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
}


def load_census() -> list[dict]:
    path = os.path.join(HERE, "census.toml")
    if not os.path.exists(path):
        raise SystemExit(f"error: {path} is missing; a census with no registry is a count")
    with open(path, "rb") as handle:
        return tomllib.load(handle)["item"]


def census_hits(
    registry: list[dict], signals: dict[str, list[tuple[str, str, str]]]
) -> dict:
    """Maps each signal onto the `TZ-nn` items.

    Four signals, because a schema message is the only one of them that shows up
    for every defect class this path has:

    `message`    a libxml2 schema violation in our output. The same rule
                 `xsd_gate.registry_hits` uses, kept separate because the two
                 registries describe different things: `XS-nn` is what our
                 writer emits for Strict input, `TZ-nn` is what the byte-level
                 normalizer leaves in a part it does not regenerate.
    `extension`  a node in the `extension` basket - a namespace the ECMA set does
                 not declare, which MCE processing removes and which therefore
                 is not a schema violation. It is still ours to clean up: a
                 Strict package is defined after MCE, so shipping the markup and
                 relying on the consumer to strip it is shipping the document
                 un-normalized.
    `dropped`    a part the input package carried and our output does not. This
                 is the signal that catches the *silent* losses - `docProps/app.xml`,
                 embedded fonts, `stylesWithEffects.xml` - none of which any schema
                 can complain about, because a part that is absent is valid.
    `unaccounted`  a dropped part the write's OWN loss report does not name. This
                 is the signal the audit §8 meant when it called the embedded
                 fonts "a silent loss": a dropped part is only a *defect* when
                 nothing says so. A part the write names is a decision it
                 reported; a part it does not is a loss it hid. Counting `dropped`
                 alone would keep every deliberate decision on the list forever.
    `lossy`      a `Lossy` record in the normalizer's own report.

    A message matching no item is counted as unmatched and printed: a defect with
    no name is one nobody is looking for.
    """
    counts = {item["id"]: 0 for item in registry}
    unmatched: list[tuple[str, str, str]] = []
    for signal, rows in signals.items():
        for where, label, detail in rows:
            for item in registry:
                if item.get("signal", "message") != signal:
                    continue
                if signal == "message":
                    hit = label in item["elements"] and any(
                        pattern in detail for pattern in item.get("messages", [])
                    )
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
                else:
                    hit = any(marker in detail for marker in item["elements"])
                if hit:
                    counts[item["id"]] += 1
                    break
            else:
                # Only a `message` can be an unnamed defect. A dropped part, an
                # extension node and a lossy record each name themselves.
                if signal == "message":
                    unmatched.append((where, label, detail))
    return {"counts": counts, "unmatched": unmatched}


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
    """Parts the input package carried and our output does not.

    The measurement the audit §8 called the only way to see a silent loss: a
    part that is *absent* validates perfectly, so no schema message exists and
    only a part-by-part comparison finds it. `word/media/image1.png` becomes
    `image2.png` on the way out, so names are normalized (`image17.png` and
    `image4.png` are the same part) and only the *shape* of what went missing is
    reported.

    One class is excluded, and the reason is the one thing a part-by-part
    comparison cannot see on its own: **a `.rels` beside a part the writer
    regenerates is a replacement, not a loss.** `word/_rels/footnotes.xml.rels`
    describes relationships the source's footnotes had; the written footnotes are
    the model's, and whatever relationships *they* have, this write declares in
    its own rels part — or declares none, because the model has none. Either way
    the source's copy is superseded, and keeping it would leave the package
    pointing at relationships the written part does not use.

    The test is whether the **owner** is still in the output, not whether a rels
    part is: the writer is free to emit no `.rels` at all for a part that ended
    up with no relationships, and a check that required one would report that as
    a loss on every document in the corpus and make the signal useless.
    """
    with zipfile.ZipFile(source) as package:
        before = {normalize_part(name) for name in package.namelist() if not name.endswith("/")}
    with zipfile.ZipFile(written) as package:
        after = {normalize_part(name) for name in package.namelist() if not name.endswith("/")}
    missing = before - after
    return collections.Counter(
        name
        for name in missing
        if not (name.endswith(".rels") and rels_owner(name) in after)
    )


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
            os.remove(out)
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
    # No `--written` here, and the absence is deliberate rather than a missing
    # feature. The `unaccounted` signal is measured against the write's OWN loss
    # report, so a run handed a directory of packages somebody else produced has
    # no report to measure against - and it would then call every dropped part
    # unaccounted, which is a number about nothing wearing the costume of a
    # number about something. `xsd_gate.py` takes `--written` because its signals
    # are all read off the bytes; the ones here are not.
    #
    # Named explicitly so the refusal is legible rather than argparse's
    # "unrecognized argument": a reader who assumed the two gates take the same
    # options should be told which flag is the difference and why.
    parser.add_argument("--written", help=argparse.SUPPRESS)
    args = parser.parse_args(argv)
    if args.written:
        raise SystemExit(
            "error: census_gate.py measures the write's own loss report, so it cannot judge "
            "packages it did not write.\n"
            "       It rebuilds the writer and writes its own corpus; use --cli to name a "
            "binary,\n       or xsd_gate.py --written for the Strict-input gate."
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
    if args.written:
        written_root = args.written
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
    lost_picture_parts: list[str] = []
    total_in = total_out = 0
    clean = 0
    documents = 0
    refused: list[tuple[str, str]] = []
    lossy_documents = 0
    lossy_total = 0
    lost_pictures = 0

    for label, corpus in CORPORA.items():
        if not os.path.isdir(corpus):
            raise SystemExit(f"error: census corpus {corpus} is not there")
        destination = os.path.join(written_root, label)
        reports: dict[str, str] = {}
        if not os.path.isdir(destination):
            count, problems, reports = write_transitional(corpus, destination, cli)
            for name, why in problems:
                refused.append((name, why))
        else:
            count = len([n for n in os.listdir(destination) if n.endswith(".docx")])

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
                refused.append((name, "our writer wrote nothing"))
                print(f"  {name:<42} {sum(incoming.schema.values()):>5} {'refused':>9}")
                label_in += sum(incoming.schema.values())
                continue
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
        if count == 0:
            state = "closed"
        elif origin == "ours":
            state = f"OPEN  {count}"
        elif origin == "source":
            state = f"CARRIED {count}"
        else:
            state = f"n/a    {count}"
        print(f"  {item['id']:<7} {origin:<7} {signal:<9} {state:<12} {item['summary']}")
    if hits["unmatched"]:
        print(f"\n=== {len(hits['unmatched'])} violation(s) match no census item")
        for where, local, message in hits["unmatched"][:40]:
            print(f"  {where} [{local}]: {message}")

    if not args.quiet_messages and out_schema:
        print("\n=== every message, so nothing is counted on trust")
        for where, _, message in out_messages:
            print(f"  {where}: {message}")

    open_items = {item_id: counts[item_id] for item_id in sorted(ours) if counts[item_id]}
    if open_items:
        print("\nFAIL: the Transitional path leaves these open: "
              + ", ".join(f"{key}={value}" for key, value in open_items.items()))
        print(f"      ({lossy_documents}/{documents} document(s) also carry a lossy record)")
        return EXIT_OPEN
    print("\nPASS: every census item of ours is closed on a measured zero")
    print(f"      ({documents - lossy_documents}/{documents} document(s) report no lossy record; "
          f"{lossy_total} lossy record(s) total - a removal the report names is not a defect, "
          "an un-named one would be)")
    return EXIT_OK


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
