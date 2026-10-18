# strict-ooxml-latex

Bounded LaTeX import into editable Strict WordprocessingML and OMML. Success is
separate from fidelity: inspect `LatexReport.loss_count` and `feature_counts`.
`UnsupportedPolicy::Reject` refuses any diagnosed loss.

## Token substitution and packages

- Ordinary `def`/`gdef`, `newcommand`/`renewcommand`/`providecommand`, robust
  commands and math operators; up to nine undelimited parameters, optional first
  argument, Unicode single-token arguments, nested expansion and local scopes.
- `let` snapshots a command's current meaning, including native formatting
  commands. `newenvironment`/`renewenvironment` substitute begin/end templates.
- `newif`, its true/false assignments, `iftrue`/`iffalse` and nested boolean
  branches. Other TeX conditionals receive a loss and their branches are skipped.
- Local `.sty` files load through `SourceProvider`, once per package name.
  Packages requiring TeX primitives are declined atomically with a named loss,
  so partial interpretation cannot redefine native importer commands.
  Internal `@` names are tokenized inside packages and `makeatletter` regions.
  Preamble `input` files share the macro state with the document.
- Existing built-in importer handlers supply explicitly named subsets of common
  packages such as amsmath, graphicx, hyperref, array and booktabs. A package being
  named does not mean its complete TeX implementation is supported.

Expanded tokens, structural nesting, input bytes and included files remain
bounded. Direct recursive definitions and definitions using unsupported TeX
primitives (assignment hooks, csname/expandafter, register arithmetic, etc.) are
diagnosed rather than executed. Mutual expansion cycles reach a typed limit.
Delimited primitive parameters, arbitrary catcodes, edef/xdef evaluation and
expl3 are outside this substitution engine.

## Native text commands and explicit groups

`textunderscore`, the local `em` declaration and native long arrows (`longrightarrow`,
`longleftarrow`, `longleftrightarrow`), `mid` and `top` preserve their characters.
`em` toggles italic within the current scope, including nested emphasis.

Inside OMML text, `textbf`, `textit`, `textup`, `textnormal`, nested text groups,
`emph` and ordinary bold/italic declarations compose without printing TeX syntax.
Escaped braces, underscore, percent, dollar, hash and ampersand, nonbreaking spaces,
Unicode text and nested `$...$` / `ensuremath` are preserved. Normal-text font
properties are written as ordinary `w:rPr`; unknown text commands remain explicit
losses. This is a bounded subset of text formatting, not general TeX execution.

`begingroup` / `endgroup` restore local macros, booleans, character formatting,
named colors, graphics paths, column types and environment definitions. Groups
may span paragraphs and included files; `gdef` definitions survive restoration.
Brace groups and environments also restore local assignments. Unmatched or
unterminated explicit groups are reported, and all grouping obeys nesting limits.

## Metadata, columns and references

`elsarticle` front matter preserves title, all authors, numbered affiliations,
email/URL links and correspondence/title notes. Symbolic affiliation/note keys
are metadata. Correspondence notes use ordinary numbered Word footnotes.
The publisher's exact page design and symbol style are not reproduced.

Tables accept parameterized `newcolumntype`, repeated packs and column
declarations `>{...}` / `<{...}`. Supported font declarations are scoped to the
cell; centering/ragged alignment and arraybackslash are handled. Intercolumn
material (`@{...}`, `!{...}`), unknown column types and unresolved tabularx target
widths are explicit losses. This does not execute arbitrary TeX column code or
extend mathematical-array layout to every tabular feature.

Forward `ref`/`eqref`/`nameref` caches resolve from the importer's section,
equation, figure and table counters and captured titles. Aligned equation rows receive individual
counter values. `.aux` can supply cached prior page/reference values; the file is
read as data, without executing its commands. Prior TeX page numbers may differ
from Word pagination. Unknown values become `??` with a loss, never raw keys.
The caches are not a TeX or Word field-update engine; custom counter schemes,
custom float/theorem numbering styles and class-specific reference formats are not
fully evaluated.

The importer reads `.bbl` bibliography entries and resolves citations forward;
tagged bibinfo/bibfield metadata and first/second-argument selectors retain only
the displayed content;
natbib short-author/year labels exclude the trailing long-author metadata.
When `.bbl` is absent it can read literal `.bib` author/title/publication/year/
DOI/URL fields and emit a normalized bibliography, explicitly reporting the style
change. Arbitrary `.bst`, biber, BibTeX string expressions/crossref inheritance,
and exact textual citation-style semantics are not implemented.

## Opaque graphics and PDF resources

Default `import` / `import_file` start no external tools. They diagnose unsupported
graphics and consume TikZ/chemfig source as an opaque unit, without printing its
program into document prose. An embedding application opts into conversion with
`RenderingSource` and `GraphicRenderer`:

```rust,ignore
use strict_ooxml_latex::{FsSource, RenderingSource, TectonicRenderer,
    import_with_source, LatexOptions};
use std::{path::PathBuf, time::Duration};

let root = PathBuf::from("document-directory");
let source = RenderingSource {
    source: FsSource::new(&root),
    renderer: TectonicRenderer {
        tectonic: PathBuf::from("absolute-path-to-tectonic.exe"),
        pdftoppm: PathBuf::from("absolute-path-to-pdftoppm.exe"),
        temporary_root: std::env::temp_dir().join("latex-graphics"),
        timeout: Duration::from_secs(30),
        only_cached: true,
    },
};
let bytes = std::fs::read(root.join("main.tex"))?;
let imported = import_with_source("main.tex", &bytes, &source,
    &LatexOptions::default())?;
```

The supplied adapter supports TikZ environments/inline commands, chemfig (also
inside a formula) and the first page of PDF images. It compiles TeX with Tectonic
`--untrusted`, then uses Poppler to create a PNG. Processes have a timeout and no
shell invocation/escape; Windows console windows are hidden. The raster's longest
side is bounded to 4096 pixels. PNG density preserves physical figure dimensions.
Returned media sizes also obey the import limits.

Rasterization is a named **Loss**: the result is a picture, with diagram/formula
editability lost. Reject policy refuses this approximation. Missing engines,
uncached packages or conversion failures remain explicit losses; source code
does not replace the picture. Engine binaries/packages are not bundled with the
crate. Network package fetching is possible only if the caller explicitly sets
`only_cached: false`.

The adapter preserves the original preamble apart from the document class and
uses standalone for a cropped graphic. Its temporary job does not automatically
copy project-local assets/packages or implement every source class. Applications
needing that context can implement `GraphicRenderer` with their own resource
staging and execution policy. Other unsupported containers, such as SVG/EPS,
also need a corresponding renderer. MuFoEd's import route has not yet been wired
to this optional external renderer.

## Verification

```powershell
cargo +1.92.0 test -p strict-ooxml-latex --offline
cargo +1.92.0 clippy -p strict-ooxml-latex --all-targets --offline -- -D warnings
```

`tests/extensions.rs` contains portable regressions. Its single ignored test is
a real external-tool integration check, not a mocked renderer. To run it, provide
`LATEX_TEST_TECTONIC`, `LATEX_TEST_PDFTOPPM` and a populated Tectonic cache (the
test uses `only_cached: true`):

```powershell
cargo +1.92.0 test -p strict-ooxml-latex --test extensions real_tikz_and_chemfig_are_embedded_as_rendered_pictures --offline -- --ignored --nocapture
```

The test creates actual TikZ/chemfig pictures, a DOCX and a PDF conversion probe
under `target/latex-extended-probes/`; it checks the PDF's physical dimensions.
Corpus evidence and remaining limitations are recorded in
`docs/audit-latex-2026-10-18/EXTENSIONS.md`.
