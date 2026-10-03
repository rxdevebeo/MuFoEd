#!/usr/bin/env python3
"""Self-test for the OPC gate (AUD-21).

The gate must be able to fail: a package written before AUD-20 with a purl
`.rels` namespace produces > 0 violations. Exit 0 only when that failure is
observed.
"""

from __future__ import annotations

import os
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)

import opc_gate  # noqa: E402


def main() -> int:
    fixture = os.path.join(HERE, "fixtures", "purl-rels.docx")
    if not os.path.exists(fixture):
        print(f"error: missing fixture {fixture}", file=sys.stderr)
        return 2
    config = opc_gate.load_config()
    schema_dir = opc_gate.resolve_schema_dir(config)
    schemas = opc_gate.compile_schemas(schema_dir)
    violations = opc_gate.validate_package(fixture, schemas)
    print(f"fixture: {fixture}")
    print(f"violations: {len(violations)}")
    for line in violations[:20]:
        print(f"  {line}")
    if len(violations) == 0:
        print(
            "error: purl-rels.docx produced zero violations; the gate cannot fail",
            file=sys.stderr,
        )
        return 1
    print("PASS: gate rejects legacy purl .rels")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
