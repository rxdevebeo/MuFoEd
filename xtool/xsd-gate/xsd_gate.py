#!/usr/bin/env python3
"""The XSD gate: what we write, measured against the official ECMA-376 Strict schemas.

`STAGE-10G-TASK.md`, requirements G1-G16. Read that file before changing this one:
almost every awkward thing here is there for a stated reason, and the reasons are
load-bearing rather than historical.

What it does, in order:

  1. finds the schemas - `STRICT_XSD_DIR`, else the user's cache, else downloads
     the ECMA archive and verifies its SHA-256 (G1, G7);
  2. writes a PATCHED COPY into the cache beside the pristine extraction, and
     prints the diff of the patch the first time (G3, G4, G6);
  3. compiles one driver schema per part root, because the ECMA set declares no
     global elements at all - only types and local `xsd:element` (G11);
  4. REFUSES TO PRINT NUMBERS if a single schema failed to compile (G9);
  5. validates every `.xml` part of every corpus document, on input and on the
     output of our own writer, over the ASSEMBLED PACKAGE (G14), classifying each
     message into one of three baskets (G13);
  6. prints the counts, the skipped-part counter, the uncovered-root counter and
     which `XS-nn` are still open (G10, G15).

Three baskets, and the third one is not optional
-----------------------------------------------
`schema`         a violation on a node the ECMA set declares. This is ours.
`extension`      a violation on a node from a namespace the ECMA set does not
                 declare - `w14`, `wps`, `wp14`. An MCE processor removes exactly
                 these, and ECMA-376 Part 1 §2.1 clause (ii) defines Strict
                 conformance on the POST-MCE part, so they are not schema
                 defects of the document (ADR-0014).
`mce-artifact`   a violation caused by the MCE markup itself. It cannot appear in
                 a correct run because `mce_process()` runs first and removes
                 the markup - which is the point: the basket exists so that a
                 run which silently skipped MCE processing would be caught
                 rather than reported as clean.

Why a Python oracle and not a crate
------------------------------------
There is no XSD validator in the Rust ecosystem, and writing one costs more than
the whole order; a native libxml2 binding is a native dependency, and the project
has refused those (ADR-0009). So this is a tool and a CI step, not a feature: it
adds nothing to the crate graph, and `Cargo.toml` is untouched by it.

Usage
-----
    python xtool/xsd-gate/xsd_gate.py                      # the whole gate
    python xtool/xsd-gate/xsd_gate.py --corpus DIR         # another corpus
    python xtool/xsd-gate/xsd_gate.py --written DIR        # packages already written
    python xtool/xsd-gate/xsd_gate.py --keep-written DIR   # keep what we write

Exit codes: 0 clean, 1 schema violations, 2 the harness could not measure.
"""

from __future__ import annotations

import argparse
import collections
import difflib
import hashlib
import io
import os
import re
import shutil
import subprocess
import sys
import tempfile
import urllib.request
import zipfile

try:
    from lxml import etree
except ImportError:  # pragma: no cover - the gate's own dependency
    sys.stderr.write(
        "error: the XSD gate needs lxml, which is not in the crate graph and is "
        "not vendored here.\n"
        "       install it with `pip install -r xtool/xsd-gate/requirements.txt`, "
        "or point STRICT_XSD_DIR at an unpacked schema set.\n"
    )
    raise SystemExit(2)

try:
    import tomllib
except ModuleNotFoundError:  # pragma: no cover - Python < 3.11
    sys.stderr.write("error: the XSD gate needs Python 3.11 or newer (tomllib)\n")
    raise SystemExit(2)

HERE = os.path.dirname(os.path.abspath(__file__))
REPO = os.path.dirname(os.path.dirname(HERE))
VERSION = "xsd-gate 1.0.0"

XSDNS = "http://www.w3.org/2001/XMLSchema"
MC = "http://schemas.openxmlformats.org/markup-compatibility/2006"
XML_LANG = "http://www.w3.org/XML/1998/namespace"

# libxml2 names the offending node by qualified name in one of three shapes:
# Clark notation `{ns}local` when it resolved the namespace, `prefix:local` when
# it kept the prefix, and bare `local` when there is no namespace at all. The
# first is authoritative; the second needs the document's own prefix bindings.
RE_ELEMENT_TEXT = re.compile(r"Element '([^']+)'")
RE_ATTRIBUTE_TEXT = re.compile(r"attribute '([^']+)'")
QUALIFIED = re.compile(r"^(?:\{(?P<ns>[^}]*)\})?(?:(?P<prefix>[A-Za-z_][\w.\-]*):)?(?P<local>[^\s]+)$")


def split_qualified(text: str) -> tuple[str | None, str | None]:
    """`{ns}local` / `prefix:local` / `local` -> (namespace-or-prefix, local name)."""
    match = QUALIFIED.match(text.strip())
    if not match:
        return None, None
    namespace = match.group("ns")
    if namespace is not None:
        return namespace, match.group("local")
    prefix = match.group("prefix")
    return prefix, match.group("local")

EXIT_OK = 0
EXIT_VIOLATIONS = 1
EXIT_UNMEASURABLE = 2


# --------------------------------------------------------------------------
# 1. Where the schemas come from
# --------------------------------------------------------------------------


def load_config() -> dict:
    with open(os.path.join(HERE, "schemas.toml"), "rb") as handle:
        config = tomllib.load(handle)
    config["our_xml_xsd"] = os.path.join(HERE, "xml.xsd")
    return config


def cache_root() -> str:
    base = os.environ.get("XDG_CACHE_HOME") or os.path.join(
        os.environ.get("LOCALAPPDATA") or os.path.expanduser("~"), "cache"
    )
    return os.path.join(base, "strict-ooxml", "xsd-gate")


def extract_pristine(archive: bytes, destination: str, config: dict) -> None:
    """Unpacks the Strict schema set, byte for byte, into `destination`."""
    os.makedirs(destination, exist_ok=True)
    outer = zipfile.ZipFile(io.BytesIO(archive))
    inner = zipfile.ZipFile(io.BytesIO(outer.read(config["source"]["member"])))
    names = [n for n in inner.namelist() if n.endswith(".xsd")]
    if len(names) != config["source"]["expected_xsd"]:
        raise SystemExit(
            f"error: the archive holds {len(names)} XSDs, the config expects "
            f"{config['source']['expected_xsd']}. The oracle is not the one this gate was "
            "written against, and reporting numbers from it would be a lie."
        )
    for name in names:
        with open(os.path.join(destination, os.path.basename(name)), "wb") as handle:
            handle.write(inner.read(name))


def fetch_archive(config: dict, destination: str) -> bytes:
    """Downloads the ECMA archive and verifies its checksum before using a byte."""
    url = config["source"]["url"]
    path = os.path.join(destination, "ecma376-part1.zip")
    if os.path.exists(path) and os.path.getsize(path) == config["source"]["bytes"]:
        with open(path, "rb") as handle:
            data = handle.read()
    else:
        print(f"downloading {url} ...", flush=True)
        request = urllib.request.Request(url, headers={"User-Agent": "strict-ooxml-xsd-gate"})
        try:
            with urllib.request.urlopen(request, timeout=600) as response:
                data = response.read()
        except OSError as error:
            raise SystemExit(
                f"error: cannot fetch the ECMA archive ({error}).\n"
                "       Without the schemas there is no gate, and a gate that "
                "measures nothing must not pass.\n"
                "       Unpack OfficeOpenXML-XMLSchema-Strict.zip by hand and point\n"
                "       STRICT_XSD_DIR at it to run offline."
            )
    digest = hashlib.sha256(data).hexdigest()
    if len(data) != config["source"]["bytes"] or digest != config["source"]["sha256"]:
        raise SystemExit(
            f"error: the archive does not match the pinned SHA-256.\n"
            f"       expected {config['source']['sha256']}\n"
            f"       actual   {digest} ({len(data)} bytes)\n"
            "       Refusing to validate against an oracle nobody pinned."
        )
    with open(path, "wb") as handle:
        handle.write(data)
    return data


def write_patched_copy(pristine: str, fixed: str, config: dict) -> list[str]:
    """Writes the patched copy into the cache and returns the diff, line by line.

    Three corrections, each forced by an observed failure:

    1. The `default="off"` on three `s:ST_OnOff` attributes is not a Strict value
       (`ST_OnOff` is `union(xsd:boolean)` in Strict; `on`/`off` live only in the
       Transitional set). Without this `wml.xsd` does not compile, and a harness
       that skips the file it cannot compile skips every part it exists to judge.
    2. The XML-namespace import exists but carries no `schemaLocation`, so
       libxml2 looks in its own catalog and resolves nothing. The import is given
       a location - our own `xml.xsd`, which is a complement, not an edit.
    3. The patch has to exist ON DISK. An in-memory-only edit was the first
       design and it does not work: libxml2 resolves schema `import`s through its
       own catalog, which lxml's `Resolver` cannot intercept. Hence the patched
       copy in the user's cache with the pristine extraction left beside it -
       which is what the licence permits and the repository must not hold.

    The patch is TEXTUAL, on the bytes ECMA shipped. A first version re-serialised
    the parsed tree, and the resulting diff showed the whole file rewritten -
    re-quoted declarations, reordered attributes, reflowed imports - which is
    not a reviewable record of a patch, and it turns the claim that the ECMA
    bytes are verbatim apart from three attributes into something a reader has
    to take on faith. String edits keep the diff to a handful of lines, and the
    diff is what makes the claim checkable rather than asserted.
    """
    os.makedirs(fixed, exist_ok=True)
    spec = config["patch"]
    diff: list[str] = []

    for name in sorted(os.listdir(pristine)):
        if not name.endswith(".xsd"):
            continue
        with open(os.path.join(pristine, name), encoding="utf-8") as handle:
            original = handle.read()

        patched = original
        if name == spec["file"]:
            for attribute in spec["attributes"]:
                needle = f' name="{attribute}" type="s:ST_OnOff" use="optional" default="{spec["default"]}"'
                replacement = f' name="{attribute}" type="s:ST_OnOff" use="optional"'
                if patched.count(needle) != 1:
                    raise SystemExit(
                        f"error: {name} does not carry exactly one `default=\"{spec['default']}\"` "
                        f"on `{attribute}` (found {patched.count(needle)}).\n"
                        "       The published schema set changed under the gate; stop and look "
                        "before the oracle silently becomes a different one."
                    )
                patched = patched.replace(needle, replacement)
                diff.append(f'- {needle.strip()}')
                diff.append(f'+ {replacement.strip()}')

        patched = give_the_xml_import_a_location(patched, name)
        if patched != original:
            diff.extend(
                line
                for line in unified_diff_text(original, patched, f"pristine/{name}", f"patched/{name}")
            )
        with open(os.path.join(fixed, name), "w", encoding="utf-8", newline="") as handle:
            handle.write(patched)

    shutil.copyfile(config["our_xml_xsd"], os.path.join(fixed, "xml.xsd"))
    diff.append("+ xml.xsd (ours): the four attributes the XML Namespaces spec fixes")
    with open(os.path.join(fixed, "PATCHES.txt"), "w", encoding="utf-8") as handle:
        handle.write("patched copy generated by " + VERSION + "\n")
        handle.write("pristine extraction: ../pristine (byte for byte)\n")
        handle.write("our complement:     xml.xsd (not ECMA content)\n\n")
        handle.write("\n".join(diff) + "\n")
    return diff


def give_the_xml_import_a_location(text: str, name: str) -> str:
    """Points the XML-namespace import at our `xml.xsd`, or adds the import.

    The import is there and empty: three schemas reference `xml:space`/`xml:lang`
    by `ref=`, libxml2 resolves `import` through its own catalog rather than
    lxml's `Resolver`, and it resolves nothing, so those schemas do not compile.
    """
    needle = f'<xsd:import namespace="{XML_LANG}"/>'
    if needle in text:
        return text.replace(
            needle, f'<xsd:import namespace="{XML_LANG}" schemaLocation="xml.xsd"/>', 1
        )
    if XML_LANG in text and 'ref="xml:' in text:
        anchor = text.index("<xsd:import")
        end = text.index("\n", anchor) + 1
        return (
            text[:end]
            + f'  <xsd:import namespace="{XML_LANG}" schemaLocation="xml.xsd"/>\n'
            + text[end:]
        )
    return text


def unified_diff_text(before: str, after: str, fromfile: str, tofile: str) -> list[str]:
    return [
        line.rstrip("\n")
        for line in difflib.unified_diff(
            before.splitlines(keepends=True),
            after.splitlines(keepends=True),
            fromfile=fromfile,
            tofile=tofile,
            n=1,
        )
    ]


def locate_schemas(config: dict) -> str:
    """Returns the directory holding the patched schema set, preparing it if needed."""
    override = os.environ.get("STRICT_XSD_DIR")
    if override:
        if not os.path.isdir(override):
            raise SystemExit(f"error: STRICT_XSD_DIR points at {override}, which is not a directory")
        # An offline set is used as given: patching it would be modifying a
        # deliverable in place, which is the one thing the notice forbids.
        if os.path.exists(os.path.join(override, "xml.xsd")):
            print(f"schemas: STRICT_XSD_DIR {override} (used as given, not patched)")
            return override
        return _stage_overlay(override, config)

    cache = cache_root()
    fixed = os.path.join(cache, "fixed")
    pristine = os.path.join(cache, "pristine")
    if os.path.exists(os.path.join(fixed, "PATCHES.txt")):
        print(f"schemas: cache {fixed}")
        return fixed

    os.makedirs(cache, exist_ok=True)
    archive = fetch_archive(config, cache)
    extract_pristine(archive, pristine, config)
    print(f"schemas: pristine extraction in {pristine} (untouched)")
    diff = write_patched_copy(pristine, fixed, config)
    print(f"schemas: patched copy in {fixed}; the patch, which is what the gate runs:")
    for line in diff:
        print(f"  {line}")
    print("  + our xml.xsd, the four attributes the XML Namespaces spec fixes (not ECMA)")
    return fixed


def _stage_overlay(source: str, config: dict) -> str:
    """Copies an offline set into the cache and patches the copy."""
    cache = cache_root()
    pristine = os.path.join(cache, "pristine")
    fixed = os.path.join(cache, "fixed")
    if not os.path.exists(os.path.join(fixed, "PATCHES.txt")):
        os.makedirs(pristine, exist_ok=True)
        for name in sorted(os.listdir(source)):
            if name.endswith(".xsd"):
                shutil.copyfile(os.path.join(source, name), os.path.join(pristine, name))
        diff = write_patched_copy(pristine, fixed, config)
        print(f"schemas: patched a copy of {source} into {fixed}; the patch:")
        for line in diff:
            print(f"  {line}")
    return fixed


# --------------------------------------------------------------------------
# 2. Drivers: the ECMA set declares no global elements
# --------------------------------------------------------------------------


class Oracle:
    """The compiled schema set: a validator per part root, and an honest account
    of the roots that have none."""

    def __init__(self, directory: str):
        self.directory = directory
        self.schemas: dict[tuple[str, str], etree.XMLSchema] = {}
        self.uncovered: list[str] = []
        self.target_namespaces: set[str] = set()
        self.failures: list[str] = []
        self.drivers = 0
        self._build()

    def _build(self) -> None:
        index: dict[str, dict[str, tuple[str, str, bool]]] = {}
        files = sorted(f for f in os.listdir(self.directory) if f.endswith(".xsd"))
        for name in files:
            tree = etree.parse(os.path.join(self.directory, name))
            root = tree.getroot()
            namespace = root.get("targetNamespace")
            if not namespace:
                continue
            self.target_namespaces.add(namespace)
            top_level = {etree.QName(child).localname for child in root if isinstance(child.tag, str)}
            for element in root.iter(f"{{{XSDNS}}}element"):
                local, kind = element.get("name"), element.get("type")
                if local and kind and local not in index.setdefault(namespace, {}):
                    index[namespace][local] = (kind, os.path.join(self.directory, name), "element" in top_level)

        work = tempfile.mkdtemp(prefix="strict-xsd-gate-drivers-")
        for namespace, roots in index.items():
            for local, (kind, source, global_decl) in sorted(roots.items()):
                key = (namespace, local)
                if not global_decl and ":" in kind:
                    # A driver can only declare an element whose type lives in the
                    # same target namespace. A prefixed type means the element is
                    # declared somewhere this set does not include; naming it is
                    # honest, mangling it into a type name that does not exist is
                    # the alternative and it produced green numbers that meant
                    # nothing.
                    self.uncovered.append(f"{os.path.basename(source)}::{local}")
                    continue
                try:
                    self.schemas[key] = self._compile(kind, local, namespace, source, global_decl, work)
                    self.drivers += 1
                except etree.XMLSchemaParseError as error:
                    self.failures.append(f"{os.path.basename(source)}::{local}: {str(error)[-160:]}")

        shutil.rmtree(work, ignore_errors=True)

    def _compile(self, kind, element, namespace, source, global_decl, work) -> etree.XMLSchema:
        if global_decl:
            return etree.XMLSchema(etree.parse(source))
        tree = etree.parse(source)
        prefix = next((p for p, uri in tree.getroot().nsmap.items() if uri == namespace and p), "x")
        driver = os.path.join(work, f"driver-{len(self.schemas)}-{safe(element)}.xsd")
        with open(driver, "w", encoding="utf-8") as handle:
            handle.write(
                '<xsd:schema xmlns:xsd="%s" xmlns:%s="%s" targetNamespace="%s"'
                ' elementFormDefault="qualified">'
                '<xsd:include schemaLocation="%s"/>'
                '<xsd:element name="%s" type="%s:%s"/></xsd:schema>'
                % (
                    XSDNS,
                    prefix,
                    namespace,
                    namespace,
                    os.path.abspath(source).replace("\\", "/"),
                    element,
                    prefix,
                    kind,
                )
            )
        return etree.XMLSchema(etree.parse(driver))


def safe(name: str) -> str:
    """A file name that cannot collide and cannot escape the driver directory."""
    return "".join(character if character.isalnum() else "_" for character in name)


# --------------------------------------------------------------------------
# 3. MCE, then validation
# --------------------------------------------------------------------------


def mce_process(root: etree._Element) -> None:
    """Applies the MCE processing that Strict conformance is defined after.

    ECMA-376 Part 1 §2.1 clause (ii): *"After the removal of any extensions by an
    MCE processor ... the part is valid against the strict W3C XML Schema."* So the
    part to validate is the one a processor leaves behind, and doing this in the
    gate is what stops `w14:paraId` and `mc:Ignorable` from being reported as
    schema defects of ours when they are not (ADR-0014).

    Three rules, in the order MCE gives them:

      - `mc:AlternateContent` resolves to its `Fallback`: an XSD processor
        understands no namespace, so no `Choice` is selectable;
      - elements in an ignorable namespace are removed, unless they carry
        `mc:ProcessContent`, which lifts their content in their place;
      - the `mc:` markup itself goes, on every element.
    """
    while True:
        for element in list(root.iter(f"{{{MC}}}AlternateContent")):
            fallback = element.find(f"{{{MC}}}Fallback")
            replacement = list(fallback) if fallback is not None else []
            parent = element.getparent()
            index = list(parent).index(element)
            tail = element.tail
            for offset, child in enumerate(replacement):
                parent.insert(index + offset, child)
            if replacement and tail:
                replacement[-1].tail = (replacement[-1].tail or "") + tail
            parent.remove(element)
            break
        else:
            break

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

    for element in list(root.iter()):
        if not isinstance(element.tag, str):
            continue
        namespace = etree.QName(element).namespace
        if namespace in ignorable:
            process_content = element.get(f"{{{MC}}}ProcessContent")
            parent = element.getparent()
            if parent is not None and process_content:
                index = list(parent).index(element)
                for offset, child in enumerate(list(element)):
                    parent.insert(index + offset, child)
            if parent is not None:
                parent.remove(element)
            continue
        for name in list(element.attrib):
            if name.startswith("{") and name[1:].split("}")[0] in ignorable | {MC}:
                del element.attrib[name]


# Parts that are OPC, not ECMA-376 Part 1 (§6 of the order): checked elsewhere,
# and calling them covered here would be a lie about the gate's reach.
OUT_OF_SCOPE_SUFFIXES = (".rels",)


class Measurement:
    def __init__(self):
        self.schema = collections.Counter()
        self.extension = collections.Counter()
        self.mce = collections.Counter()
        self.skipped: list[str] = []
        self.messages: list[tuple[str, str, str]] = []


def validate_package(path: str, oracle: Oracle) -> Measurement:
    measurement = Measurement()
    with zipfile.ZipFile(path) as package:
        for name in sorted(package.namelist()):
            if name == "[Content_Types].xml" or name.endswith(OUT_OF_SCOPE_SUFFIXES):
                measurement.skipped.append(f"{name} (OPC, not ECMA-376 Part 1)")
                continue
            if not name.endswith((".xml", ".vml")):
                continue
            try:
                root = etree.fromstring(package.read(name))
            except etree.XMLSyntaxError as error:
                measurement.skipped.append(f"{name} (not well-formed: {str(error)[-60:]})")
                continue
            qname = etree.QName(root)
            key = (qname.namespace, qname.localname)
            schema = oracle.schemas.get(key)
            if schema is None:
                why = "root type is prefixed; no driver can declare it"
                if key in {(None, key[1])}:
                    why = "no target namespace"
                measurement.skipped.append(f"{name} ({qname.localname}: {why})")
                continue

            mce_process(root)
            if schema.validate(etree.ElementTree(root)):
                continue
            lines = line_elements(root)
            for entry in schema.error_log:
                about = offender(entry, lines)
                bucket = classify(about.namespace, oracle)
                counter = getattr(measurement, bucket)
                # The ELEMENT is the name that groups a defect family: a bad
                # `w:w` on `w:top` inside `w:tblCellMar` and the same bad `w:w` on
                # a paragraph's `w:ind` are one item in the registry and two
                # different places to fix.
                counter[about.element] += 1
                if bucket == "schema":
                    measurement.messages.append(
                        (f"{name}:{entry.line}", about.element, entry.message)
                    )
    return measurement


class Offender:
    """Which node a libxml2 message is about.

    `element` is the node the count is filed under; `namespace` is the namespace
    that decides the basket, which for an attribute violation is the ATTRIBUTE's
    namespace and not the element's - libxml2 reports `w14:paraId` against the
    `w:p` that carries it, and filing that under the element would put a Microsoft
    extension into the schema basket.
    """

    __slots__ = ("element", "attribute", "namespace")

    def __init__(self, element: str, attribute: str | None, namespace: str | None):
        self.element = element
        self.attribute = attribute
        self.namespace = namespace


def line_elements(root: etree._Element) -> dict[int, list[etree._Element]]:
    """Line -> the nodes that start there.

    A line number is the only handle libxml2 gives on which node a message is
    about. It is a coarse one: a producer may put a whole part on a single line,
    so a line maps to SEVERAL nodes and the message's own qualified name is what
    actually identifies the offender.
    """
    mapping: dict[int, list[etree._Element]] = {}
    for element in root.iter():
        if isinstance(element.tag, str) and element.sourceline is not None:
            mapping.setdefault(element.sourceline, []).append(element)
    return mapping


def offender(entry, lines: dict[int, list[etree._Element]]) -> Offender:
    """Which node a libxml2 message is about.

    An ATTRIBUTE violation is reported against the element that carries the
    attribute, so the element's namespace is not the answer - `w14:paraId` on a
    perfectly legal `w:p` arrives as a message about `w:p`. The attribute's own
    qualified name is read out of the message and decides the basket; the element
    names the family the count belongs to.
    """
    element_name, element_namespace = _name_of(RE_ELEMENT_TEXT, entry)
    attribute_match = RE_ATTRIBUTE_TEXT.search(entry.message)
    if attribute_match:
        attribute_namespace, attribute_name = _resolve(attribute_match.group(1), entry, lines)
        if attribute_namespace:
            return Offender(element_name or "?", attribute_name, attribute_namespace)
        return Offender(element_name or "?", attribute_name, element_namespace)
    return Offender(element_name or entry.message[:48], None, element_namespace)


def _name_of(pattern, entry) -> tuple[str | None, str | None]:
    match = pattern.search(entry.message)
    if not match:
        return None, None
    namespace, local = split_qualified(match.group(1))
    if namespace and not namespace.startswith("http"):
        namespace = None
    return local, namespace


def _resolve(text: str, entry, lines) -> tuple[str | None, str | None]:
    namespace, local = split_qualified(text)
    if namespace and not namespace.startswith("http"):
        namespace = _resolve_prefix(namespace, lines.get(entry.line, []))
    return namespace, local


def _resolve_prefix(prefix: str, elements: list[etree._Element]) -> str | None:
    for element in elements:
        uri = element.nsmap.get(prefix)
        if uri:
            return uri
    return None


def classify(namespace: str | None, oracle: Oracle) -> str:
    """The three baskets of G13, decided by the node the message is about.

    A node from a namespace the ECMA set does not declare is an extension by
    definition: an MCE processor removes it, and conformance is defined after that
    removal (ECMA-376 Part 1 §2.1 clause ii). Nothing else is excused - a node in
    an ECMA namespace is ours to get right.
    """
    if namespace is None or namespace == XML_LANG:
        return "schema"
    if namespace == MC:
        return "mce"
    return "schema" if namespace in oracle.target_namespaces else "extension"


# --------------------------------------------------------------------------
# 4. The corpus, and what our writer made of it
# --------------------------------------------------------------------------


def write_corpus(corpus: str, destination: str, cli: str) -> tuple[int, int, list]:
    """Runs our own `write` over every document, over the ASSEMBLED package.

    The gate judges the package, not the model: an extension can arrive through
    an `Opaque*` node or through pass-through, and a gate that looked at the model
    would never see it (G14).

    The CLI's exit code is not the question either. `write` exits 1 when the
    package was written but the report has losses, and it has already written the
    file by then - a package that dropped a construct is exactly the package this
    gate most needs to look at. So a run counts as measured when the file is there
    and readable, and exit code 2 (nothing written) is the only refusal.
    """
    os.makedirs(destination, exist_ok=True)
    written = 0
    lossy = 0
    refused = []
    for name in sorted(os.listdir(corpus)):
        if not name.endswith(".docx"):
            continue
        out = os.path.join(destination, name)
        if os.path.exists(out):
            os.remove(out)
        result = subprocess.run(
            [cli, "write", os.path.join(corpus, name), "--out", out],
            capture_output=True,
            text=True,
        )
        if result.returncode == 2 or not os.path.exists(out):
            refused.append((name, (result.stderr or result.stdout).strip().splitlines()[:1]))
            continue
        written += 1
        if result.returncode != 0:
            lossy += 1
    return written, lossy, refused


# --------------------------------------------------------------------------
# 5. Controls: an instrument that cannot fail is not an instrument
# --------------------------------------------------------------------------


CONTROLS = {
    "valid": (
        "a minimal w:document, which must come out clean",
        b'<?xml version="1.0"?><w:document xmlns:w="http://purl.oclc.org/ooxml/wordprocessingml/main">'
        b"<w:body><w:p><w:r><w:t>ok</w:t></w:r></w:p></w:body></w:document>",
        0,
    ),
    "invalid": (
        "a w:pPr with w:pBdr before w:tabs, which xsd:sequence forbids - the defect "
        "this gate exists to find, and the one the project made most of",
        b'<?xml version="1.0"?><w:document xmlns:w="http://purl.oclc.org/ooxml/wordprocessingml/main">'
        b"<w:body><w:p><w:pPr>"
        b'<w:pBdr><w:top w:val="single" w:sz="4" w:space="0" w:color="auto"/></w:pBdr>'
        b'<w:tabs><w:tab w:val="left" w:pos="720"/></w:tabs>'
        b"</w:pPr><w:r><w:t>x</w:t></w:r></w:p></w:body></w:document>",
        "nonzero",
    ),
    "extension": (
        "a w:p carrying w14:paraId with mc:Ignorable declared, which an MCE processor "
        "removes and the gate must therefore NOT count as a schema violation",
        b'<?xml version="1.0"?><w:document xmlns:w="http://purl.oclc.org/ooxml/wordprocessingml/main"'
        b' xmlns:w14="http://schemas.microsoft.com/office/word/2010/wordml"'
        b' xmlns:mc="http://schemas.openxmlformats.org/markup-compatibility/2006" mc:Ignorable="w14">'
        b'<w:body><w:p w14:paraId="1A2B3C4D" w14:textId="1A2B3C4D"><w:r><w:t>x</w:t></w:r></w:p></w:body></w:document>',
        0,
    ),
    "no-mce-declared": (
        "the same w:p with w14:paraId and NO mc:Ignorable: still an extension, because "
        "conformance is defined after MCE and the declaration changes nothing about the "
        "namespace. If this one ever counted as ours, the basket would be deciding on "
        "politeness rather than on the standard",
        b'<?xml version="1.0"?><w:document xmlns:w="http://purl.oclc.org/ooxml/wordprocessingml/main"'
        b' xmlns:w14="http://schemas.microsoft.com/office/word/2010/wordml">'
        b'<w:body><w:p w14:paraId="1A2B3C4D"><w:r><w:t>x</w:t></w:r></w:p></w:body></w:document>',
        0,
    ),
}


def run_controls(oracle: Oracle) -> bool:
    """Four probes the gate must pass before its numbers mean anything.

    A validator that reports zero for an invalid document reports zero for
    everything, and the first version of this harness had exactly that defect in
    it: it caught `XMLSchemaParseError` and passed, and consequently reported a
    corpus clean that it had not measured - `wml.xsd` was the schema that would not
    compile, so `document.xml`, `styles.xml`, `numbering.xml`, `settings.xml`,
    `hdr` and `ftr` were never validated, which is precisely the set this gate
    exists to judge.
    """
    schema = oracle.schemas.get(("http://purl.oclc.org/ooxml/wordprocessingml/main", "document"))
    if schema is None:
        print("control: FAILED - no compiled schema for w:document", file=sys.stderr)
        return False
    ok = True
    for name, (description, payload, wanted) in CONTROLS.items():
        root = etree.fromstring(payload)
        mce_process(root)
        lines = line_elements(root)
        clean = schema.validate(etree.ElementTree(root))
        schema_count = 0 if clean else sum(
            1 for entry in schema.error_log if classify(offender(entry, lines).namespace, oracle) == "schema"
        )
        passed = (schema_count == 0) if wanted == 0 else (schema_count > 0)
        print(
            f"control: {'ok  ' if passed else 'FAIL'} {name}: {schema_count} schema violation(s)"
            f" - {description}"
        )
        ok = ok and passed
    return ok


# --------------------------------------------------------------------------
# 6. The report
# --------------------------------------------------------------------------


def load_registry() -> list[dict]:
    path = os.path.join(HERE, "registry.toml")
    if not os.path.exists(path):
        return []
    with open(path, "rb") as handle:
        return tomllib.load(handle)["item"]


def registry_hits(registry: list[dict], messages: list[tuple[str, str, str]]) -> dict:
    """Maps the measured messages onto the `XS-nn` items of the registry.

    An item closes when its number is zero, and only then - reading the diff
    never closes one (G17). Each recorded message is one violation, so counting
    messages is counting violations; an item matches on the offending element's
    local name plus a substring of libxml2's message, because the element alone
    cannot tell `<w:ind>` inside a cell margin from `<w:ind>` inside a paragraph.

    Anything that matches no item is reported as unmatched rather than dropped:
    a violation with no name is an unfixed defect nobody is looking for.
    """
    open_items = {item["id"]: 0 for item in registry}
    unmatched = 0
    for _, local, message in messages:
        for item in registry:
            if local in item["elements"] and any(
                pattern in message for pattern in item.get("messages", [])
            ):
                open_items[item["id"]] += 1
                break
        else:
            unmatched += 1
    return {"counts": open_items, "unmatched": unmatched}


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(description="Validate written Strict packages against ECMA-376.")
    parser.add_argument("--corpus", default=os.path.join(REPO, "strict-ooxml-core", "tests", "strict"))
    parser.add_argument("--written", help="directory of packages already written by us")
    parser.add_argument("--keep-written", help="write into this directory and keep it")
    parser.add_argument("--cli", help="path to the strict-ooxml binary")
    parser.add_argument("--quiet-messages", action="store_true", help="omit the per-message list")
    args = parser.parse_args(argv)

    config = load_config()
    print("=" * 78)
    print(f"{VERSION}")
    print(f"schema:  {config['source']['name']} ({config['source']['edition']})")
    print(f"         sha256 {config['source']['sha256']}  ({config['source']['bytes']} bytes)")
    # The validator's own version goes in the run text too (G16): an oracle whose
    # answers moved between two libxml2 builds would look like a document defect,
    # and the only way to tell the two apart is to have written down which one ran.
    print(f"oracle:  lxml {etree.LXML_VERSION}, libxml2 {etree.LIBXML_VERSION}, python {sys.version.split()[0]}")
    print(f"corpus:  {args.corpus}")


    directory = locate_schemas(config)
    oracle = Oracle(directory)

    # G9: an instrument that may not report from a partial run. The first version
    # of this harness caught XMLSchemaParseError and passed, and consequently
    # reported a corpus clean that it had not measured - wml.xsd failed to compile,
    # so document.xml, styles.xml, numbering.xml, settings.xml, hdr and ftr were
    # never validated, which is exactly the set this gate exists to judge.
    if oracle.failures:
        print(f"\nSCHEMAS THAT FAILED TO COMPILE ({len(oracle.failures)}):", file=sys.stderr)
        for failure in oracle.failures:
            print(f"  {failure}", file=sys.stderr)
        print(
            "\nrefusing to report numbers from a partial harness: a schema that did not\n"
            "compile takes its whole part set down with it, and a count that silently\n"
            "omits those parts is a count of nothing.",
            file=sys.stderr,
        )
        return EXIT_UNMEASURABLE
    print(f"schemas: {oracle.drivers} drivers compiled, {len(oracle.failures)} failures")
    print(f"roots:   {len(oracle.uncovered)} declared UNCOVERED (prefixed root type, G12)")

    if not run_controls(oracle):
        print("\ncontrol: FAILED - the gate cannot tell valid from invalid", file=sys.stderr)
        return EXIT_UNMEASURABLE

    keep = args.keep_written
    temporary = None
    if args.written:
        written_dir = args.written
    else:
        cli = args.cli or find_cli()
        destination = keep or tempfile.mkdtemp(prefix="strict-xsd-gate-written-")
        if not keep:
            temporary = destination
        print(f"writing: {cli}")
        count, lossy, refused = write_corpus(args.corpus, destination, cli)
        print(
            f"writing: {count} package(s) written into {destination}"
            + (f"; {lossy} of them with a non-empty loss report" if lossy else "")
        )
        for name, why in refused:
            print(f"writing: REFUSED {name}: {why[0] if why else ''}")
        written_dir = destination
    try:
        return report(args, oracle, written_dir)
    finally:
        if temporary:
            shutil.rmtree(temporary, ignore_errors=True)


def find_cli() -> str:
    name = "strict-ooxml.exe" if os.name == "nt" else "strict-ooxml"
    built = os.path.join(REPO, "target", "release", name)
    if os.path.exists(built):
        return built
    debug = os.path.join(REPO, "target", "debug", name)
    if os.path.exists(debug):
        return debug
    print("building the writer under test ...", flush=True)
    subprocess.run(["cargo", "build", "-p", "strict-ooxml-cli"], cwd=REPO, check=True)
    return built


def report(args, oracle: Oracle, written_dir: str) -> int:
    registry = load_registry()
    documents = sorted(n for n in os.listdir(args.corpus) if n.endswith(".docx"))
    print(f"\n{'document':<44} {'IN':>5} {'OUT':>5} {'delta':>7}")
    totals = [0, 0]
    skipped = 0
    refused = 0
    out_schema = collections.Counter()
    out_messages: list[tuple[str, str, str]] = []
    per_document = []

    for name in documents:
        incoming = validate_package(os.path.join(args.corpus, name), oracle)
        path = os.path.join(written_dir, name)
        if not os.path.exists(path):
            print(f"  {name:<42} {sum(incoming.schema.values()):>5} {'refused':>9}")
            refused += 1
            totals[0] += sum(incoming.schema.values())
            skipped += len(incoming.skipped)
            per_document.append((name, totals, None))
            continue
        outgoing = validate_package(path, oracle)
        totals[0] += sum(incoming.schema.values())
        totals[1] += sum(outgoing.schema.values())
        skipped += len(incoming.skipped) + len(outgoing.skipped)
        out_schema.update(outgoing.schema)
        out_messages.extend(outgoing.messages)
        print(
            f"  {name:<42} {sum(incoming.schema.values()):>5} {sum(outgoing.schema.values()):>5}"
            f" {sum(outgoing.schema.values()) - sum(incoming.schema.values()):>+7}"
        )
        per_document.append((name, None, outgoing))

    print(f"  {'TOTAL':<42} {totals[0]:>5} {totals[1]:>5} {totals[1] - totals[0]:>+7}")
    print(f"\nparts not covered by the ECMA set (G10): {skipped} part(s), and "
          f"{len(oracle.uncovered)} root(s) with no driver - both listed, never counted green")
    if refused:
        print(f"packages our writer refused outright: {refused}")

    print(f"\n=== our output, by element: {sum(out_schema.values())} violation(s), "
          f"{len(out_schema)} distinct element(s)")
    for local, count in out_schema.most_common(60):
        print(f"  {local:<20} {count}")

    hits = registry_hits(registry, out_messages)
    counts = hits["counts"]
    ours = {item["id"] for item in registry if item["origin"] == "ours"}
    print("\n=== registry (`XS-nn` closes on a measured zero, never on a code review)")
    for item in registry:
        count = counts[item["id"]]
        origin = item["origin"]
        if count == 0:
            state = "closed"
        elif origin == "ours":
            state = f"OPEN  {count}"
        elif origin == "source":
            state = f"CARRIED {count}"
        else:
            state = f"n/a    {count}"
        print(f"  {item['id']:<7} {origin:<9} {state:<12} {item['summary']}")
    if hits["unmatched"]:
        print(f"  {hits['unmatched']} violation(s) match no registry item and are named above")

    if not args.quiet_messages and out_schema:
        print("\n=== every message, so nothing is counted on trust")
        for where, _, message in out_messages:
            print(f"  {where}: {message}")

    print(f"\nuncovered roots ({len(oracle.uncovered)}), by schema:")
    by_schema = collections.Counter(item.split("::")[0] for item in oracle.uncovered)
    for name, count in sorted(by_schema.items()):
        print(f"  {name}: {count}")

    our_total = sum(counts[item_id] for item_id in ours)
    carried = sum(
        count for item_id, count in counts.items() if item_id not in ours and count
    )
    if our_total:
        print(f"\nFAIL: {our_total} schema violation(s) in our own output")
        return EXIT_VIOLATIONS
    print("\nPASS: no schema violation of ours in the written corpus")
    if carried:
        print(
            f"      ({carried} carried from the producer's own markup - pass-through parts "
            f"the writer does not model. XS-16, closed by G18's mark rather than by a repair.)"
        )
    return EXIT_OK


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
