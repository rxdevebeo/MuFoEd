# strict-ooxml

Rust toolkit for reading, editing, rendering and writing **WordprocessingML
Strict** documents (ISO/IEC 29500-1:2008), with controlled normalization of
Transitional (ISO/IEC 29500-4:2008) input, import from PDF, DjVu, RTF, LaTeX
and HTML, and **MuFoEd**, a desktop editor built on it.

The design documents, gates and decision records are indexed in
[`docs/README.md`](docs/README.md); the specification is
[`docs/TZ-STRICT-OOXML-RUST.md`](docs/TZ-STRICT-OOXML-RUST.md).

## Workspace

### Library

| Crate | Purpose |
|---|---|
| `strict-ooxml` | Public API: `StrictDocument`, open/convert/render/write, the editor facade. |
| `strict-ooxml-core` | OPC/ZIP, XML, namespaces, resource limits, errors, Transitional normalization. |
| `strict-ooxml-wml` | WordprocessingML Strict model (DOM) and event-driven parser. |
| `strict-ooxml-edit` | Transactional editing over the model; caret and hit map over the placed pages. |
| `strict-ooxml-report` | Feature Report: what a document uses and what is supported (JSON schema, text). |
| `strict-ooxml-render-svg` | Deterministic layout and SVG rendering. |
| `strict-ooxml-render-pdf` | PDF output over the same layout, with embedded subset fonts and real text. |
| `strict-ooxml-write` | Serialization of the model back to a Strict `.docx`, with a loss report. |
| `strict-ooxml-pdf` | PDF reading (text with geometry, fonts, images, vector paths) and the rasterizer of the pixel gate. |
| `strict-ooxml-convert` | PDF and DjVu to Strict DOCX: re-flowed (`semantic`), line by line (`visual`), or set as the original laid it out. |
| `strict-ooxml-djvu` | Bounded DjVu reader with text zones and optional page images. |
| `strict-ooxml-ocr` | Optional recognition of scanned pages through a local Ollama vision model. |
| `strict-ooxml-rtf`, `strict-ooxml-latex`, `strict-ooxml-html` | Bounded RTF, LaTeX and HTML-fragment import into the model. |
| `strict-ooxml-fidelity` | The pixel gate's arithmetic: SSIM, ink profiles and the policy they are held to. |
| `strict-ooxml-testkit` | Test-only builders for synthetic and hostile inputs. |

### Applications

| Crate | Purpose |
|---|---|
| `mufoed` | MuFoEd (MultiFormatEditor): a desktop editor (egui) for DOCX, with PDF/DjVu/RTF/LaTeX/HTML open and import, spelling, printing and an optional local AI assistant. |
| `chem-editor`, `diagram-editor`, `mathml-editor` | Visual editors for chemistry, shapes and diagrams, and MathML, embedded in MuFoEd. |
| `strict-ooxml-cli` | The `strict-ooxml` command-line tool. |
| `strict-ooxml-view` | Local HTTP viewer for rendered documents. |
| `xtool` | Development utility: XSD inventory, coverage gate, corpus fetch, `.docx` generator. |

## CLI

```text
strict-ooxml inspect      <file.docx>
strict-ooxml check        <file.docx> [--transitional]          # exit 0 / 1 / 2
strict-ooxml report       <file.docx> [--json|--text] [--out <path>]
strict-ooxml render       <file.docx> [--out <dir|page.svg>] [--pages 1-3] [--scale 96]
strict-ooxml to-pdf       <file.docx> --out <file.pdf>
strict-ooxml write        <file.docx> --out <file.docx> [--transitional]
strict-ooxml normalize    <file.docx>                           # Loss Report for Transitional
strict-ooxml from-pdf     <file.pdf>  --out <file.docx> [--pages 1-10] [--ocr]
strict-ooxml from-djvu    <file.djvu> --out <file.docx>
strict-ooxml djvu-dump    <file.djvu>
strict-ooxml import-rtf   <file.rtf>  --out <file.docx>
strict-ooxml import-latex <file.tex>  --out <file.docx>
strict-ooxml fonts        list | install <pack...> | needed <file.docx>
strict-ooxml lexicons     list | install <id...> | remove <id...> | path
```

`check` exits `0` when no `unsupported`/`error` blocker is found, `1` for a
blocker (or Transitional input under `StrictOnly`) and `2` for damaged input.
`render` writes deterministic SVG pages; `to-pdf` writes PDF with the used faces
subsetted and embedded, so the text is selectable. `write` serializes the parsed
model back to a Strict package and exits `1` when something was lost; every loss
is named in the report.

**Fonts.** The bundled faces draw Latin, Greek, Cyrillic, Hebrew, Arabic and
Tibetan. Chinese, Japanese, Korean, the Indic scripts, Thai, Lao, Khmer, Myanmar
and Ethiopic need a font pack: `fonts list` shows the catalog (Noto, SIL OFL 1.1),
`fonts install <pack...>` downloads packs pinned by SHA-256, and `render`/`to-pdf`
use the installed packs a document needs. Nothing is downloaded unless asked
(`--fetch-fonts` on a render; `--no-font-packs` to render without them).

**Scans.** A scanned PDF or DjVu has no text to convert. `from-pdf --ocr` (a build
with `--features ocr-ollama`) shows each page without text to a local Ollama vision
model (`OLLAMA_HOST`, `OLLAMA_MODEL`) and writes what it reads as paragraphs in the
`Recovered` style; `--ocr-lang <tag>` restores the diacritics a model drops from
an installed word-list pack (`lexicons install`). DjVu import prefers the native text layer; see
[`docs/DJVU_IMPORT.md`](docs/DJVU_IMPORT.md).

**Off-page text.** Text a document places outside the sheet is drawn where the
model puts it, as Word and WPS do; every such fragment is recorded on
`Page::off_page`. `--off-page-marks` tags it in the output and `--off-page=clamp`
pulls it back onto the page.

**Hostile input.** `ResourceLimits` (DOCX) and `PdfLimits` (PDF) bound ZIP sizes
and ratios, XML depth, block, inline, text-box and math nesting, and for PDF the
inflated stream sizes, per-page content and the picture and form caches. Every
line the CLI prints escapes control characters from document text.

## MuFoEd

```text
cargo run -p mufoed --release
```

Opens DOCX, and converts PDF, DjVu, RTF, LaTeX and HTML into the editing model on
open (a PDF opens set as the original laid it out; *Import* offers the re-flowed
and per-line modes, a page range and recognition). The optional AI assistant and
scan recognition talk to a local Ollama server only.

## Build and test

```text
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
cargo doc --workspace --no-deps
```

CI pins Rust 1.92.0 (`rust-version` in `Cargo.toml`). It also runs, among others:

- the two pixel gates over pinned WPS references, through the SVG and the PDF
  renderer:
  `cargo test -p strict-ooxml-render-svg --all-features --test ssim` and
  `cargo test -p strict-ooxml-pdf --features raster --test pdf_pixels`;
- the schema gates against the official ECMA-376 Strict set
  (`python xtool/xsd-gate/xsd_gate.py`, `census_gate.py`; requirements in
  `xtool/xsd-gate/requirements.txt`);
- line coverage with `cargo llvm-cov`: `core` and `wml` ≥ 85%, `render-svg` ≥ 82%,
  `report`, `fidelity` and `write` ≥ 80%, `pdf` and `render-pdf` ≥ 75%,
  `convert` ≥ 70%;
- `cargo deny check` and a nightly fuzz smoke run (see
  [`docs/fuzz-protocol.md`](docs/fuzz-protocol.md)).

What each gate checks and how a waiver is granted:
[`docs/GATE-STRATEGY.md`](docs/GATE-STRATEGY.md),
[`docs/waivers.toml`](docs/waivers.toml).

**Corpus.** The CC0 test corpus is named by `testdata-lock/cc0.toml`; the
documents are not committed. Fetch the CI tier once and the tests that need it
run instead of skipping:

```text
cargo run -p xtool --release -- corpus fetch --tier ci-core   # or ci-full
STRICT_OOXML_CORPUS=require cargo test --workspace --all-features
```

**Looking at a page** instead of a number, for either backend:

```text
cargo run -p strict-ooxml-render-svg --example page_diff -- <document> <page>
cargo run -p strict-ooxml-pdf --features raster --example pdf_page_diff -- <document> <page>
```

## License

MIT OR Apache-2.0. Vendored and bundled third-party components keep their own
licences (`vendor/`, [`docs/ATTRIBUTION-vendor-hayro.md`](docs/ATTRIBUTION-vendor-hayro.md),
the bundled fonts). “DjVu” is a descriptive format reference; this project is
not affiliated with or endorsed by its trademark owners.
