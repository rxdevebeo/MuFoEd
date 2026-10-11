#!/usr/bin/env python3
"""Compare our `NAVM` outline against `djvused`'s `print-outline` (SC-D9 L3).

Our producer (`strict-ooxml-djvu --example outline`) prints the outline as an
S-expression; the oracle is `djvused -e print-outline`. The driver parses both
with a generic S-expression reader and compares the **ordered (title, url)
pairs** of the pre-order forest — the tree order is the one thing both writers
agree on, so this is robust to each writer's exact wrapper and spacing.

`print-outline` prints nothing when a file has no `NAVM`, so the CC0 corpus
(no outlines) is inventory-only; the gate runs on the synthetic fixture from
`xtool/djvu-outline-fixture.py` (built with `djvused set-outline`).

`djvused` is GPL, invoked as a separate process only.

Usage:
    python3 xtool/djvu-outline-oracle.py --corpus DIR --oracle /usr/bin/djvused \\
        --report out.json [--ours target/release/examples/outline] [--resume]
"""
from __future__ import annotations

import argparse
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import djvu_oracle_common as common  # noqa: E402


def tokenize(text: str) -> list:
    """Split an S-expression into `(`, `)`, and string/atom tokens."""
    tokens: list = []
    at, end = 0, len(text)
    while at < end:
        char = text[at]
        if char.isspace():
            at += 1
        elif char in "()":
            tokens.append(char)
            at += 1
        elif char == '"':
            at += 1
            value = []
            while at < end:
                char = text[at]
                if char == "\\" and at + 1 < end:
                    value.append(text[at + 1])
                    at += 2
                elif char == '"':
                    at += 1
                    break
                else:
                    value.append(char)
                    at += 1
            tokens.append(("str", "".join(value)))
        else:
            start = at
            while at < end and not text[at].isspace() and text[at] not in '()"':
                at += 1
            tokens.append(("atom", text[start:at]))
    return tokens


def parse(tokens: list) -> list:
    """Parse tokens into a list of top-level forms (nested lists and strings)."""
    pos = 0

    def one():
        nonlocal pos
        token = tokens[pos]
        if token == "(":
            pos += 1
            items = []
            while pos < len(tokens) and tokens[pos] != ")":
                items.append(one())
            if pos < len(tokens):
                pos += 1  # consume ")"
            return items
        if isinstance(token, tuple):
            pos += 1
            return token[1]
        pos += 1
        return None

    forms = []
    while pos < len(tokens):
        forms.append(one())
    return forms


def _walk(node, out: list) -> None:
    """Collect `("title" "url" …)` nodes in pre-order, ignoring any wrapper."""
    if not isinstance(node, list):
        return
    if len(node) >= 2 and isinstance(node[0], str) and isinstance(node[1], str):
        out.append((node[0], node[1]))
        rest = node[2:]
    else:
        rest = node
    for element in rest:
        _walk(element, out)


def outline_nodes(text: str) -> list:
    nodes: list = []
    for form in parse(tokenize(text)):
        _walk(form, nodes)
    return nodes


def oracle_outline(djvused: Path, source: Path) -> tuple[list | None, str]:
    proc = common.run([str(djvused), "-e", "print-outline", str(source)])
    if proc.returncode != 0:
        return None, f"djvused print-outline exit {proc.returncode}"
    return outline_nodes(proc.stdout.decode("utf-8", errors="replace")), "ok"


def our_outline(producer: Path, source: Path) -> tuple[list | None, str]:
    proc = common.run([str(producer), str(source)])
    if proc.returncode != 0:
        return None, f"producer exit {proc.returncode}"
    return outline_nodes(proc.stdout.decode("utf-8", errors="replace")), "ok"


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--corpus", type=Path, required=True)
    parser.add_argument("--oracle", type=Path, required=True, help="the djvused binary")
    parser.add_argument("--report", type=Path, required=True)
    parser.add_argument("--ours", type=Path, default=None, help="optional outline producer")
    parser.add_argument("--resume", action="store_true")
    args = parser.parse_args()

    previous = common.load_previous(args.report) if args.resume else {}
    results = []
    total_oracle = 0
    total_ours = 0
    for source in common.iter_corpus(args.corpus):
        digest = common.sha256_file(source)
        old = previous.get(source.name)
        if common.reused(old, digest):
            results.append(old)
            total_oracle += len(old.get("oracle_outline") or [])
            total_ours += len(old.get("our_outline") or [])
            continue
        oracle, detail = oracle_outline(args.oracle, source)
        row = {
            "file": source.name,
            "sha256": digest,
            "oracle_nodes": len(oracle) if oracle is not None else None,
            "oracle_detail": detail,
        }
        if oracle is not None:
            total_oracle += len(oracle)
        if args.ours is not None:
            ours, producer_detail = our_outline(args.ours, source)
            row["our_nodes"] = len(ours) if ours is not None else None
            if producer_detail != "ok":
                row["producer_detail"] = producer_detail
            if ours is not None:
                total_ours += len(ours)
            row["ok"] = ours == oracle
        else:
            row["ok"] = oracle is not None
        print(source.name, row.get("oracle_nodes"), flush=True)
        results.append(row)

    report = {
        "criterion": "SC-D9 L3",
        "status": "open" if args.ours is None else "gated",
        "what": "NAVM outline titles/urls vs djvused print-outline (ordered pairs)",
        "oracle": common.fingerprint(args.oracle, "djvulibre-bin"),
        "oracle_nodes_total": total_oracle,
        "our_nodes_total": total_ours if args.ours is not None else 0,
        "files": len(results),
        "ok": sum(1 for row in results if row["ok"]),
        "mismatched": [row["file"] for row in results if not row["ok"]],
        "results": results,
    }
    common.write_report(args.report, report)
    print({"files": report["files"], "oracle_nodes": total_oracle, "status": report["status"]})
    if args.ours is None:
        return 0
    return 0 if report["ok"] == report["files"] else 1


if __name__ == "__main__":
    common.main_guard(main)
