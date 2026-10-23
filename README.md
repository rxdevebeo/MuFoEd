# MuFoEd — a modular desktop application constructor

**MuFoEd (MultiFormatEditor)** brings reusable Rust libraries together in one
desktop application. Its purpose is to let developers assemble the tools a
particular user needs: document editing, image creation, galleries, file
management and local AI, in a shared interface.

Start with the user's workflow, choose the libraries that cover it, and connect
them through the application host. A writer, an illustrator and someone managing
a document archive can use different compositions of the same building blocks.
The current MuFoEd application is an integrated composition of these components,
with documents, images, the gallery and the file manager sharing one window.

## Assemble an application for the user

| User workflow | Building blocks |
|---|---|
| Writing and working with documents | StrictLib document toolkit, formula, diagram and chemistry editors. |
| Creating and editing images | Image Studio, layers and manual tools, optional image generation and processing models. |
| Organizing visual assets | Image gallery, previews, metadata and batch image operations. |
| Managing files and archives | Two-panel file manager, search, document previews and media playback. |
| Combining these workflows | The MuFoEd host, shared tabs, settings, localization and model runtime. |

Composition currently happens in source and at build time: choose Cargo
dependencies and available features, then integrate component APIs and UI into
the host. The checked-in host includes the components listed below; creating a
smaller or different edition requires changes to its dependencies and integration.
A graphical module installer or runtime plug-in marketplace is not implemented.

Components keep their own responsibilities and APIs. The host connects them to
file opening and saving, tabs, settings and background work. For AI adapters,
the shared model runtime coordinates tasks and memory budgets across text,
vision and image workloads.

See the [MuFoEd application guide](mufoed/README.md) for launch instructions,
the current composition and integration details.

## Document toolkit: StrictLib / strict-ooxml

The document subsystem is a Rust toolkit for reading, editing, rendering and
writing **WordprocessingML Strict** documents (ISO/IEC 29500-1:2008), with
controlled normalization of Transitional (ISO/IEC 29500-4:2008) input and import
from PDF, DjVu, RTF, LaTeX and HTML. Import support and fidelity depend on the
format and supported features; conversion and support reports describe losses
and limitations.

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

### Application components and tools

| Crate | Purpose |
|---|---|
| [`mufoed`](mufoed/README.md) | Application host (egui): shared tabs for documents, images, gallery and file manager, settings and integration of the component libraries. |
| `chem-editor`, `diagram-editor`, `mathml-editor` | Visual editors for chemistry, shapes and diagrams, and MathML, embedded in MuFoEd. |
| [`image-editor`](image-editor/README.md) | Image Studio: reusable layered raster editor, manual tools, PSD saving and optional Plakat generation and image-processing pipelines. |
| [`image-gallery`](image-gallery/README.md) | Embeddable image gallery with previews, metadata and batch operations. |
| [`file-manager`](file-manager/README.md) | Embeddable two-panel file manager with folder tabs, file operations, archives and previews. |
| [`mufoed-model-runtime`](model-runtime/README.md) | Shared task and memory manager for participating text, vision/OCR and image model adapters. |
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

On Windows, launch a built `target/release/mufoed.exe` directly or through a
shortcut. For development:

```text
cargo +1.92.0 run -p mufoed --release
```

The current composition opens documents and images in individual tabs, with
gallery and file manager tabs available on demand. It edits Strict DOCX and
imports PDF, DjVu, RTF, LaTeX and HTML into the document model. Image Studio adds
manual raster editing and optional model-based generation and processing.
The text assistant and scan recognition use a local Ollama server; image
generation can use an embedded Plakat backend when enabled at build time.
AI features require their model weights and applicable runtimes. See the
[application guide](mufoed/README.md) for GPU build and runtime details.

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
