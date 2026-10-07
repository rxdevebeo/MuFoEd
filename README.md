# strict-ooxml

Rust toolkit for reading, normalizing and rendering **WordprocessingML Strict**
documents (ISO/IEC 29500-1:2008), with controlled normalization of Transitional
(ISO/IEC 29500-4:2008) input.

Design documents live in the repository root:

- `TZ-STRICT-OOXML-RUST.md` — full technical specification (v2.1).
- `REWORK-AUDIT-2026-10.md` — audit close-out plan (AUD-00…AUD-94); current
  execution status and accepted deviations (ADR-0015…0019).
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
| `strict-ooxml-render-pdf` | PDF output: a second backend over the same layout (Stage 8B). |
| `strict-ooxml-pdf` | PDF reading, and the rasterizer the pixel gate measures with (Stage 8C). |
| `strict-ooxml-fidelity` | The pixel gate's arithmetic: SSIM, ink profiles, and the policy file. |
| `strict-ooxml-write` | Serialization of the model back to a Strict `.docx` (Stage 8A). |
| `strict-ooxml` | Public `StrictDocument` API (Stages 2–4, 8A). |
| `strict-ooxml-cli` | `inspect` / `check` / `report` / `render` / `write` / `normalize` command-line tool. |
| `xtool` | Dev utility: XSD inventory, coverage gate, `.docx` generator. |

## CLI

```text
strict-ooxml inspect   <file.docx>
strict-ooxml check     <file.docx> [--transitional]      # exit 0 / 1 / 2
strict-ooxml report    <file.docx> [--json|--text] [--out <path>] [--transitional]
strict-ooxml render    <file.docx> [--out <dir|page.svg>] [--pages 1-3] [--scale 96]
strict-ooxml normalize <file.docx>                       # Loss Report for Transitional
strict-ooxml to-pdf    <file.docx> --out <file.pdf>      # Stage 8B
strict-ooxml from-pdf  <file.pdf>  --out <file.docx>     # Stage 8C+
strict-ooxml write     <file.docx> --out <file.docx>     # Stage 8A
```

`check` exits `0` when no `unsupported`/`error` blocker is found, `1` for a
blocker (or Transitional input under `StrictOnly`), and `2` for damaged input.
`report` emits the full JSON Feature Report (or human text with `--text`).
`render` emits deterministic SVG pages (`1` when rendered but the report has
blockers). `to-pdf` renders PDF with the used faces subsetted and embedded, so
the text is selectable and searchable; it exits `1` when the render lost
something. `write` serializes the parsed model back to a Strict package, prints
what the writer could not express, and exits `1` when something was lost
(`STAGE-8-TASK.md`).

## PDF conversion (Stage 8)

PDF support is specified in `STAGE-8-TASK.md` and delivered in phases:

- **8A — done.** `strict-ooxml-write` + `strict-ooxml write`: DOM → Strict XML →
  OPC, deterministic, with a loss report. Unmodelled chart/SmartArt parts are
  passed through (W7); see ADR-0007 and the historical `8A-CHART` waiver record.
- **8B — done.** `strict-ooxml-render-pdf` + `strict-ooxml to-pdf`: PDF over the
  same placement as the SVG backend, with real embedded text. Pixel gate:
  `cargo test -p strict-ooxml-pdf --features raster --test pdf_pixels`.
- **8C — done.** `strict-ooxml-pdf` (`lopdf`) + `strict-ooxml-convert` +
  `strict-ooxml from-pdf` (`semantic` / `visual`). Optional OCR/classifier is
  behind crate features (`strict-ooxml-ocr`).

## Build and test

```text
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo build --workspace --all-targets --all-features
cargo test --workspace --all-features
cargo doc --workspace --no-deps
```

Corpus. The CC0 test corpus is named by `testdata-lock/cc0.toml` (the
documents are not committed; `docs/CC0_CORPUS_MIGRATION_PLAN.md`). Fetch the
small CI tier once, and tests that need it run instead of skipping:

```text
cargo run -p xtool --release -- corpus fetch --tier ci-core   # or ci-full
STRICT_OOXML_CORPUS=require cargo test --workspace --all-features
```

Coverage (≥ 80% lines for `core`, `wml`, `report`, `render-svg` and `fidelity`):

```text
cargo llvm-cov -p strict-ooxml-core       --all-features --fail-under-lines 80
cargo llvm-cov -p strict-ooxml-wml        --all-features --fail-under-lines 80
cargo llvm-cov -p strict-ooxml-report     --all-features --fail-under-lines 80
cargo llvm-cov -p strict-ooxml-render-svg --all-features --fail-under-lines 80
cargo llvm-cov -p strict-ooxml-fidelity   --all-features --fail-under-lines 80
```

The two pixel gates, over the same pinned WPS references and through two
different renderers (`CORE-QUEUE.md` §1):

```text
cargo test -p strict-ooxml-render-svg --all-features --test ssim
cargo test -p strict-ooxml-pdf --features raster --test pdf_pixels
```

Looking at a page instead of a number, for either backend:

```text
cargo run -p strict-ooxml-render-svg --example page_diff -- <document> <page>
cargo run -p strict-ooxml-pdf --features raster --example pdf_page_diff -- <document> <page>
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

The two schema gates, against the official ECMA-376 Strict set. Same oracle, two
paths — one oracle rather than two, because a second definition of "Strict" makes
every threshold a number about nothing (`GATE-STRATEGY.md` §7):

```text
pip install -r xtool/xsd-gate/requirements.txt
python xtool/xsd-gate/xsd_gate.py      # what we write from Strict input      (XS-nn)
python xtool/xsd-gate/census_gate.py   # what we write from Transitional input (TZ-nn)
```

`census_gate.py` is the one that judges `write --transitional` end to end — the
byte-level normalizer, the pass-through and the loss report — over 58 Transitional
documents. Its `unaccounted` signal fails on a part that was dropped **and not
named in the report**, which is the loss class no schema can see: a missing part
validates perfectly and draws nothing.

Fuzzing (requires `cargo-fuzz`, a nightly toolchain, and Linux — see
`docs/fuzz-protocol.md`; AUD-92):

```text
cargo +nightly fuzz run --target x86_64-unknown-linux-gnu fuzz_zip
cargo +nightly fuzz run --target x86_64-unknown-linux-gnu fuzz_xml
cargo +nightly fuzz run --target x86_64-unknown-linux-gnu fuzz_relpath
cargo +nightly fuzz run --target x86_64-unknown-linux-gnu fuzz_wml
cargo +nightly fuzz run --target x86_64-unknown-linux-gnu fuzz_normalize
cargo +nightly fuzz run --target x86_64-unknown-linux-gnu fuzz_docx_full
cargo +nightly fuzz run --target x86_64-unknown-linux-gnu fuzz_pdf
cargo +nightly fuzz run --target x86_64-unknown-linux-gnu fuzz_convert
```

## License

MIT OR Apache-2.0.
