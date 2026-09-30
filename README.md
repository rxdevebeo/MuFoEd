# strict-ooxml

Rust toolkit for reading, normalizing and rendering **WordprocessingML Strict**
documents (ISO/IEC 29500-1:2008), with controlled normalization of Transitional
(ISO/IEC 29500-4:2008) input.

Design documents live in the repository root:

- `TZ-STRICT-OOXML-RUST.md` — full technical specification.
- `base_target.md` — the original (1.0) specification.
- `STAGE-1-TASK.md`, `STAGE-1-REWORK.md` — Stage-1 task and rework.
- `STAGE-2-TASK.md`, `STAGE-2-REWORK.md` — Stage-2 task and rework.
- `STAGE-3-TASK.md` — Stage-3 task (Feature Report).
- `STAGE-4-TASK.md` — Stage-4 task (SVG rendering).
- `STAGE-5-TASK.md`, `STAGE-5B-TASK.md` — Stage-5 tasks (5A extended support,
  5B DrawingML + page borders).
- `STAGE-8-TASK.md` — Stage-8 task: PDF ↔ Strict conversion in four phases.
- `docs/stage-2-report.md`, `docs/stage-3-report.md`, `docs/stage-4-report.md`,
  `docs/stage-5-report.md`, `docs/stage-5b-report.md` — stage delivery reports.
- `docs/adr/` — accepted architecture decision records (incl. ADR-0004/0005/0006).

## Workspace layout

| Crate | Purpose |
|---|---|
| `strict-ooxml-core` | OPC/ZIP, XML, namespaces, limits, errors (Stage 1). |
| `strict-ooxml-wml` | WordprocessingML Strict DOM and parser (Stage 2). |
| `strict-ooxml-report` | Feature Report: model, build, JSON schema, text (Stage 3). |
| `strict-ooxml-render-svg` | Deterministic SVG layout and rendering (Stage 4). |
| `strict-ooxml-write` | Serialization of the model back to a Strict `.docx` (Stage 8A). |
| `strict-ooxml` | Public `StrictDocument` API (Stages 2–4, 8A). |
| `strict-ooxml-cli` | `inspect` / `check` / `report` / `render` / `write` / `normalize` command-line tool. |
| `xtool` | Dev utility: XSD inventory, coverage gate, `.docx` generator. |

## CLI

```text
strict-ooxml inspect <file.docx>
strict-ooxml check   <file.docx>                       # exit 0 / 1 / 2
strict-ooxml report  <file.docx> [--json|--text] [--out <path>]
strict-ooxml render  <file.docx> [--out <dir|page.svg>] [--pages 1-3] [--scale 96]
strict-ooxml write   <file.docx> --out <file.docx>     # Stage 8A
```

`check` exits `0` when no `unsupported`/`error` blocker is found, `1` for a
blocker (or Transitional input under `StrictOnly`), and `2` for damaged input.
`report` emits the full JSON Feature Report (or human text with `--text`).
`render` emits deterministic SVG pages (`1` when rendered but the report has
blockers). `write` serializes the parsed model back to a Strict package, prints
what the writer could not express, and exits `1` when something was lost
(`STAGE-8-TASK.md`).

## PDF conversion (Stage 8)

PDF support is specified in `STAGE-8-TASK.md` and delivered in phases:

- **8A — done.** `strict-ooxml-write` + `strict-ooxml write`: DOM → Strict XML →
  OPC, deterministic, with a loss report. Not yet done: unmodelled parts (charts,
  diagrams) are dropped rather than passed through — see the `8A-CHART` waiver
  in `docs/waivers.toml`.
- **8B–8D — specified, not implemented.** PDF output (`strict-ooxml-render-pdf`),
  PDF reading (`strict-ooxml-pdf`, built on `lopdf`), the PDF → WML converter in
  `semantic` and `visual` modes, and the optional ollama-backed OCR/figure
  classifier.

## Build and test

```text
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo build --workspace --all-targets --all-features
cargo test --workspace --all-features
cargo doc --workspace --no-deps
```

Coverage (≥ 80% lines for `core`, `wml`, `report` and `render-svg`):

```text
cargo llvm-cov -p strict-ooxml-core       --all-features --fail-under-lines 80
cargo llvm-cov -p strict-ooxml-wml        --all-features --fail-under-lines 80
cargo llvm-cov -p strict-ooxml-report     --all-features --fail-under-lines 80
cargo llvm-cov -p strict-ooxml-render-svg --all-features --fail-under-lines 80
```

Feature Report schema validation (independent `jsonschema` oracle) and
examples:

```text
cargo test -p strict-ooxml-report --all-features --test schema
cargo run  -p strict-ooxml --example support_report -- document.docx
cargo run  -p strict-ooxml --example render_svg -- document.docx out/
```

Rendering benchmark (10/100/500 pages):

```text
cargo bench -p strict-ooxml-render-svg
```

Optional-element coverage gate (≥ 90%) and the independent corpus cross-check:

```text
cargo run -p xtool -- coverage --file coverage/wml-elements.toml --min 90
cargo run -p xtool -- corpus-elements
```

Fuzzing (requires `cargo-fuzz` and a nightly toolchain):

```text
cargo +nightly fuzz run fuzz_zip
cargo +nightly fuzz run fuzz_xml
cargo +nightly fuzz run fuzz_relpath
cargo +nightly fuzz run fuzz_wml
```

## License

MIT OR Apache-2.0.
