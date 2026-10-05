# R02 ACCEPTANCE — Strict serialization of CC0 corpus

Date: 2026-10-05

## Identifiers

| Field | Value |
|---|---|
| Code HEAD | `b413b1594447458fac8101093b6502eacf4ff661` (dirty tree) |
| Toolchain | `cargo +1.92.0`, `python` + lxml XSD gate |
| CC0 manifest SHA-256 | `68b0aead98882ac1d85bdaf24eda015a6eae06b01fd1f582b2f1321e86ed5745` |
| Result | **PASS** (ours=0, unmatched=0; source XS-16 = 7 carried) |

## Baseline → after

| Metric | Before (audit evidence) | After |
|---|---:|---:|
| OUT schema messages | 39 642 | 7 (source only) |
| unmatched | 39 635 | 0 |
| ours | — | 0 |
| source (XS-16 charts) | 7 | 7 |
| CC0 fixed_point | — | 100/100 |
| XSD gate exit | 1 | 0 |

## Fixes (own writer/normalize)

1. **`w:rFonts/@w:hint="cs"`** — Strict `ST_Hint` only allows `default`/`eastAsia`; `cs` omitted (`strict-ooxml-write/src/props.rs`).
2. **`w:placeholder`** — parse `w:docPart/@w:val`; write only with `docPart`; never empty shell.
3. **`w:documentProtection`** — map Transitional crypto attrs to Strict; drop forbidden attrs; keep enforcement.
4. **`w:tblGrid`** — synthesize when rows exist and grid empty.
5. **Paragraph-mark `w:ins`/`w:del`** — not emitted inside mark `w:rPr` (illegal vs `rPrChange`); reported as unsupported.
6. **DrawingML percentages** — T4 `drawingml_thousandths_percent` (`65000`→`65%`) for tint/shade/satMod/lumMod/gs/@pos/baseline/…
7. **Math `ST_OnOff`** — `off`/`on` → `false`/`true` for on/off carriers only (not `lMargin`/`rMargin`).
8. **Empty `w:docParts` / empty `c:ext`** — dropped after ignorable children removed.

## Evidence commands

```
python docs/audit-review-2026-10-05/cc0_probe.py
python xtool/xsd-gate/xsd_gate.py --corpus testdata/CC0_DOCX --written target/cc0-acceptance-probe/written --no-build --quiet-messages
```

Logs: `target/audit-rework-2026-10-05/R02/green/`.

Final measured line:

`documents=100 validated=100 missing=0 refused=0 unmatched=0 ours=0 source=7`

`PASS: no schema violation of ours; 7 source violation(s) are carried` (XS-16 chart `lblOffset`/`gapWidth`/`overlap` from producer input).

## Limits

- 27 Strict + 121 Transitional full XSD matrices not re-run in this receipt (CC0 was the failing corpus); recommend R11 gate.
- Source chart lexical defects intentionally not “fixed” by dropping charts.
