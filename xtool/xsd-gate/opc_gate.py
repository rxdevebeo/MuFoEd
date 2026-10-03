#!/usr/bin/env python3
"""OPC gate (AUD-21): written packages against ECMA-376 Part 2 schemas.

Validates every `*.rels`, `[Content_Types].xml`, and core-properties part in
packages produced by our writer. Schemas are downloaded from ECMA-376 Part 2
and never committed (`no_ecma_bytes.py`).

    python xtool/xsd-gate/opc_gate.py
    python xtool/xsd-gate/opc_gate.py --written DIR
    OPC_XSD_DIR=/path/to/xsds python xtool/xsd-gate/opc_gate.py

Exit codes: 0 clean, 1 schema violations, 2 the harness could not measure.
"""

from __future__ import annotations

import argparse
import hashlib
import io
import os
import shutil
import subprocess
import sys
import tempfile
import urllib.request
import zipfile

try:
    from lxml import etree
except ImportError:  # pragma: no cover
    sys.stderr.write(
        "error: the OPC gate needs lxml: pip install -r xtool/xsd-gate/requirements.txt\n"
    )
    raise SystemExit(2)

try:
    import tomllib
except ModuleNotFoundError:  # pragma: no cover
    sys.stderr.write("error: the OPC gate needs Python 3.11 or newer (tomllib)\n")
    raise SystemExit(2)

HERE = os.path.dirname(os.path.abspath(__file__))
REPO = os.path.dirname(os.path.dirname(HERE))
VERSION = "opc-gate 1.0.0"

EXIT_OK = 0
EXIT_VIOLATIONS = 1
EXIT_UNMEASURABLE = 2

CORE_PROPERTIES_CT = "application/vnd.openxmlformats-package.core-properties+xml"
DC_URLS = {
    "dc.xsd": "http://dublincore.org/schemas/xmls/qdc/2003/04/02/dc.xsd",
    "dcterms.xsd": "http://dublincore.org/schemas/xmls/qdc/2003/04/02/dcterms.xsd",
    "dcmitype.xsd": "http://dublincore.org/schemas/xmls/qdc/2003/04/02/dcmitype.xsd",
}


def load_config() -> dict:
    with open(os.path.join(HERE, "opc.toml"), "rb") as handle:
        return tomllib.load(handle)


def cache_root() -> str:
    base = os.environ.get("XDG_CACHE_HOME") or os.path.join(
        os.environ.get("LOCALAPPDATA") or os.path.expanduser("~"), "cache"
    )
    return os.path.join(base, "strict-ooxml", "opc-gate")


def fetch_archive(config: dict, destination: str) -> bytes:
    url = config["source"]["url"]
    path = os.path.join(destination, "ecma376-part2.zip")
    if os.path.exists(path) and os.path.getsize(path) == config["source"]["bytes"]:
        with open(path, "rb") as handle:
            data = handle.read()
    else:
        print(f"downloading {url} ...", flush=True)
        request = urllib.request.Request(url, headers={"User-Agent": "strict-ooxml-opc-gate"})
        try:
            with urllib.request.urlopen(request, timeout=600) as response:
                data = response.read()
        except OSError as error:
            raise SystemExit(
                f"error: cannot fetch the ECMA Part 2 archive ({error}).\n"
                "       Point OPC_XSD_DIR at an unpacked OpenPackagingConventions-XMLSchema "
                "directory to run offline."
            ) from error
    digest = hashlib.sha256(data).hexdigest()
    if len(data) != config["source"]["bytes"] or digest != config["source"]["sha256"]:
        raise SystemExit(
            f"error: the Part 2 archive does not match the pinned SHA-256.\n"
            f"       expected {config['source']['sha256']}\n"
            f"       actual   {digest} ({len(data)} bytes)"
        )
    with open(path, "wb") as handle:
        handle.write(data)
    return data


def extract_schemas(archive: bytes, destination: str, config: dict) -> None:
    os.makedirs(destination, exist_ok=True)
    outer = zipfile.ZipFile(io.BytesIO(archive))
    inner = zipfile.ZipFile(io.BytesIO(outer.read(config["source"]["member"])))
    names = [name for name in inner.namelist() if name.endswith(".xsd")]
    if len(names) != config["source"]["expected_xsd"]:
        raise SystemExit(
            f"error: the archive holds {len(names)} XSDs, opc.toml expects "
            f"{config['source']['expected_xsd']}"
        )
    for name in names:
        with open(os.path.join(destination, os.path.basename(name)), "wb") as handle:
            handle.write(inner.read(name))


def fetch_dublin_core(destination: str) -> None:
    for name, url in DC_URLS.items():
        path = os.path.join(destination, name)
        if os.path.exists(path) and os.path.getsize(path) > 0:
            continue
        print(f"downloading {url} ...", flush=True)
        request = urllib.request.Request(url, headers={"User-Agent": "strict-ooxml-opc-gate"})
        with urllib.request.urlopen(request, timeout=120) as response:
            data = response.read()
        with open(path, "wb") as handle:
            handle.write(data)


def prepare_working_copy(pristine: str, working: str) -> None:
    """Copy XSDs and rewrite remote Dublin Core / xml imports to local files."""
    if os.path.isdir(working):
        shutil.rmtree(working)
    shutil.copytree(pristine, working)
    xml_xsd = os.path.join(HERE, "xml.xsd")
    shutil.copy2(xml_xsd, os.path.join(working, "xml.xsd"))
    core = os.path.join(working, "opc-coreProperties.xsd")
    text = open(core, encoding="utf-8").read()
    text = text.replace(
        'schemaLocation="http://dublincore.org/schemas/xmls/qdc/2003/04/02/dc.xsd"',
        'schemaLocation="dc.xsd"',
    )
    text = text.replace(
        'schemaLocation="http://dublincore.org/schemas/xmls/qdc/2003/04/02/dcterms.xsd"',
        'schemaLocation="dcterms.xsd"',
    )
    text = text.replace(
        '<xs:import id="xml" namespace="http://www.w3.org/XML/1998/namespace"/>',
        '<xs:import id="xml" namespace="http://www.w3.org/XML/1998/namespace" '
        'schemaLocation="xml.xsd"/>',
    )
    open(core, "w", encoding="utf-8", newline="\n").write(text)
    # dcterms.xsd imports dcmitype and dc with absolute URLs in some editions.
    for name in ("dcterms.xsd", "dc.xsd"):
        path = os.path.join(working, name)
        if not os.path.exists(path):
            continue
        body = open(path, encoding="utf-8").read()
        for remote, local in (
            ("http://dublincore.org/schemas/xmls/qdc/2003/04/02/dc.xsd", "dc.xsd"),
            ("http://dublincore.org/schemas/xmls/qdc/2003/04/02/dcterms.xsd", "dcterms.xsd"),
            ("http://dublincore.org/schemas/xmls/qdc/2003/04/02/dcmitype.xsd", "dcmitype.xsd"),
            ("http://www.w3.org/2001/xml.xsd", "xml.xsd"),
        ):
            body = body.replace(f'schemaLocation="{remote}"', f'schemaLocation="{local}"')
        open(path, "w", encoding="utf-8", newline="\n").write(body)


def resolve_schema_dir(config: dict) -> str:
    override = os.environ.get("OPC_XSD_DIR")
    if override:
        if not os.path.isdir(override):
            raise SystemExit(f"error: OPC_XSD_DIR points at {override}, which is not a directory")
        return override
    root = cache_root()
    pristine = os.path.join(root, "pristine")
    working = os.path.join(root, "working")
    stamp = os.path.join(root, "sha256")
    expected = config["source"]["sha256"]
    if not (
        os.path.isdir(working)
        and os.path.exists(stamp)
        and open(stamp, encoding="utf-8").read().strip() == expected
        and os.path.exists(os.path.join(working, "opc-relationships.xsd"))
    ):
        os.makedirs(root, exist_ok=True)
        archive = fetch_archive(config, root)
        if os.path.isdir(pristine):
            shutil.rmtree(pristine)
        extract_schemas(archive, pristine, config)
        fetch_dublin_core(pristine)
        prepare_working_copy(pristine, working)
        open(stamp, "w", encoding="utf-8").write(expected + "\n")
    return working


def compile_schemas(schema_dir: str) -> dict[str, etree.XMLSchema]:
    parsers = {}
    for key, filename in (
        ("rels", "opc-relationships.xsd"),
        ("content-types", "opc-contentTypes.xsd"),
        ("core-properties", "opc-coreProperties.xsd"),
    ):
        path = os.path.join(schema_dir, filename)
        try:
            doc = etree.parse(path)
            parsers[key] = etree.XMLSchema(doc)
        except etree.XMLSchemaParseError as error:
            raise SystemExit(
                f"error: {filename} failed to compile: {error}\n"
                "       Refusing to print violation counts from a broken oracle."
            ) from error
    return parsers


def content_types_map(package: zipfile.ZipFile) -> dict[str, str]:
    """Part name (with leading /) -> content type."""
    mapping: dict[str, str] = {}
    try:
        raw = package.read("[Content_Types].xml")
    except KeyError:
        return mapping
    root = etree.fromstring(raw)
    ns = {"ct": "http://schemas.openxmlformats.org/package/2006/content-types"}
    defaults = {
        node.get("Extension", "").lower(): node.get("ContentType", "")
        for node in root.findall("ct:Default", ns)
    }
    for node in root.findall("ct:Override", ns):
        part = node.get("PartName", "")
        mapping[part] = node.get("ContentType", "")
    for name in package.namelist():
        part = "/" + name if not name.startswith("/") else name
        if part in mapping:
            continue
        if "." in name:
            ext = name.rsplit(".", 1)[-1].lower()
            if ext in defaults:
                mapping[part] = defaults[ext]
    return mapping


def validate_package(path: str, schemas: dict[str, etree.XMLSchema]) -> list[str]:
    violations: list[str] = []
    with zipfile.ZipFile(path) as package:
        types = content_types_map(package)
        for name in package.namelist():
            if name.endswith("/"):
                continue
            part = "/" + name if not name.startswith("/") else name
            data = package.read(name)
            kind = None
            if name == "[Content_Types].xml":
                kind = "content-types"
            elif name.endswith(".rels"):
                kind = "rels"
            elif types.get(part) == CORE_PROPERTIES_CT:
                kind = "core-properties"
            if kind is None:
                continue
            try:
                doc = etree.fromstring(data)
            except etree.XMLSyntaxError as error:
                violations.append(f"{os.path.basename(path)}::{name}: XML syntax: {error}")
                continue
            schema = schemas[kind]
            if not schema.validate(doc):
                for entry in schema.error_log:
                    violations.append(
                        f"{os.path.basename(path)}::{name}: {entry.message}"
                    )
    return violations


def write_corpus(out_dir: str) -> list[str]:
    cli = os.path.join(REPO, "target", "release", "strict-ooxml")
    if os.name == "nt":
        cli += ".exe"
    if not os.path.exists(cli):
        cli_debug = cli.replace("release", "debug")
        cli = cli_debug if os.path.exists(cli_debug) else cli
    if not os.path.exists(cli):
        raise SystemExit(
            "error: strict-ooxml binary not found; build with "
            "`cargo build --release -p strict-ooxml-cli` first"
        )
    os.makedirs(out_dir, exist_ok=True)
    corpus = os.path.join(REPO, "strict-ooxml-core", "tests", "strict")
    written: list[str] = []
    for name in sorted(os.listdir(corpus)):
        if not name.endswith(".docx"):
            continue
        source = os.path.join(corpus, name)
        target = os.path.join(out_dir, name)
        subprocess.run(
            [cli, "write", source, "--out", target],
            check=False,
            capture_output=True,
        )
        if os.path.exists(target):
            written.append(target)
    return written


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--written", help="directory of already-written packages")
    parser.add_argument(
        "--keep-written",
        help="write the Strict corpus here when --written is omitted",
    )
    args = parser.parse_args(argv)

    config = load_config()
    print(f"{VERSION}")
    print(f"source:  {config['source']['name']}")
    print(f"         sha256 {config['source']['sha256']}")
    print(
        f"oracle:  lxml {etree.LXML_VERSION}, libxml2 {etree.LIBXML_VERSION}, "
        f"python {sys.version.split()[0]}"
    )

    schema_dir = resolve_schema_dir(config)
    print(f"schemas: {schema_dir}")
    schemas = compile_schemas(schema_dir)

    temp_dir = None
    if args.written:
        packages = [
            os.path.join(args.written, name)
            for name in sorted(os.listdir(args.written))
            if name.endswith(".docx")
        ]
    else:
        temp_dir = args.keep_written or tempfile.mkdtemp(prefix="opc-gate-")
        packages = write_corpus(temp_dir)
        print(f"written: {len(packages)} package(s) in {temp_dir}")

    if not packages:
        print("error: no packages to validate", file=sys.stderr)
        return EXIT_UNMEASURABLE

    violations: list[str] = []
    for path in packages:
        violations.extend(validate_package(path, schemas))

    by_signal = {"rels": 0, "content-types": 0, "core-properties": 0}
    for item in violations:
        if "::[Content_Types].xml" in item or "Content_Types" in item:
            by_signal["content-types"] += 1
        elif ".rels:" in item or item.endswith(".rels"):
            by_signal["rels"] += 1
        else:
            by_signal["core-properties"] += 1

    print(f"packages: {len(packages)}")
    print(f"violations: {len(violations)}")
    for item in config.get("item", []):
        count = by_signal.get(item["signal"], 0)
        status = "closed" if count == 0 else "OPEN"
        print(f"  {item['id']} {status:6}  {count:4}  {item['title']}")

    if violations:
        print("---")
        for line in violations[:40]:
            print(line)
        if len(violations) > 40:
            print(f"... and {len(violations) - 40} more")
        return EXIT_VIOLATIONS

    if temp_dir and not args.keep_written:
        shutil.rmtree(temp_dir, ignore_errors=True)
    print("PASS")
    return EXIT_OK


if __name__ == "__main__":
    raise SystemExit(main())
