# R03 ACCEPTANCE — Census inventory vs schema separation

Date: 2026-10-05

## Identifiers

| Field | Value |
|---|---|
| Code HEAD | `b413b1594447458fac8101093b6502eacf4ff661` (dirty tree) |
| Dirty-tree fingerprint (`git status --porcelain`) | `59650326008a445a212f224db3bd3b22316a306ecbe14db75d0af3d9aaa20078` |
| `xtool/xsd-gate/census_gate.py` SHA-256 | `7645224e9fce9bcf98cd37904508da38a879c6001c74a6363c96255f0f225f69` |
| `xtool/xsd-gate/census.toml` SHA-256 | `2b15abb270254ee6826a5d61db94cdc7dd3d78be658186120f598837b3b69f09` |
| `xtool/xsd-gate/census_gate_selftest.py` SHA-256 | `0af46bfc0ed4ed61e04cf1d894a42a57a31028c6612529451d92024f384a916b` |
| Toolchain | `python` + lxml XSD/census gate, `cargo +1.92.0` CLI for write |
| Result | **PASS** |

## Bug (RED)

Historical receipt (`docs/audit-review-2026-10-05/census.log` and re-run): 121 validated Strict outs, XSD messages=0, but census exit=1 with `unmatched=4189`. Those rows were **inventory element changes** (vanished Strict-declared locals after write), mis-reported as schema violations via a merged `census_hits` / `unmatched` path.

Reproduced RED log: `target/audit-rework-2026-10-05/R03/red/census.log`

```
measured: documents=121 validated=121 missing=0 unmatched=4189 ours=0
FAIL: unmatched=4189 schema violation(s) match no registry item
```

Log SHA-256: `891aabaebc8a2bf0b2e041c8cad0610ac8b1db10bf876b6e1be8ecef9d30d256`

Negative control (`red/misclassify_probe.txt`): production FAIL with `unclassified_element_changes` (not schema); inverse merge of inventory into schema FAIL as `unmatched` schema — proves the old classifier defect.

## Fix

1. **`census_hits`** — classify `message` vs `element` signals separately into `unmatched_schema` and `unclassified_element_changes`.
2. **Exit decision** — unknown XSD message → FAIL as schema; unnamed inventory change → FAIL as incomplete inventory (explicitly *not* a schema error); named/waived dispositions do not invent losses.
3. **Qualified matching** — inventory rows matched with preferred namespace prefixes / parent context, not bare local-name alone.
4. **`census.toml`** — dispositions TZ-22…TZ-43 for regeneration/rename/declared transforms observed on the Transitional corpus (DrawingML chrome, property bags, sect helpers, math toggles, etc.). No blanket drop of unknown inventory.
5. **`census_gate_selftest.py`** — negative controls for schema unmatched, unclassified inventory, and merged-as-schema inverse.

## GREEN

```
python xtool/xsd-gate/census_gate_selftest.py
python xtool/xsd-gate/census_gate.py --no-build --quiet-messages --keep-written target/audit-rework-2026-10-05/R03/green/census-written
```

| Check | Result |
|---|---|
| `census_gate_selftest` | pass (exit 0) |
| Transitional census 121 | `unmatched_schema=0 unclassified_element_changes=0 ours=0` → PASS |
| CC0 inventory (100 written outs) | `unclassified_distinct=0 unclassified_total=0` (schema carries = 7 source XS-16 only; see R02) |

GREEN measured line:

```
measured: documents=121 validated=121 missing=0 unmatched_schema=0 unclassified_element_changes=0 ours=0
PASS: schema-clean; inventory dispositions complete; no owned census hits open
      (91/121 document(s) report no lossy record; 43 lossy record(s) total)
```

Logs: `target/audit-rework-2026-10-05/R03/green/`  
- `census.log` SHA-256 `14d916dc54040341862b9df5cc396cabddcca940b4253f03809f8230d9461db5`  
- `selftest3.log` SHA-256 `289a33813b1b820bda2639132b21236efa6d1c763529c9e519ab05bd875bc4bc`  
- `cc0_inventory.json` — 100 docs, unclassified=0

## Inverse

`target/audit-rework-2026-10-05/R03/inverse/inverse_probe.txt`: restoring merged inventory→`unmatched` schema reporting fails with schema-style unmatched on named element changes (`left`, `Pages`), while production classifies unknown inventory as `unclassified_element_changes`.

## Limits

- Commit/push not performed (owner permission required).
- Full CC0 path is measured via XSD gate (R02) + inventory disposition probe; the default `census_gate.py` corpus remains the 121 Transitional set (CENSUS-LOCAL). Historical audit numbers are not rewritten.
- Waived TZ inventory groups document declared transforms; they are not silent unnamed losses.
