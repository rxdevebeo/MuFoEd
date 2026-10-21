# LaTeX corpus audit client

Uses public StrictLib APIs without modifying library implementation or corpus.
The corpus manifest selects the 100 main entry points; auxiliary `.tex` files
are inventoried, not counted as independent papers. Includes use the same
`import_file` provider/root semantics as the public adapter.

From the repository root, in PowerShell:

Use a new empty output directory for each run. Existing receipts are preserved;
the runner refuses to mix them with a new run.

```powershell
cargo +1.92.0 build --release --offline --manifest-path tools/latex-audit/Cargo.toml --target-dir target/latex-audit-build
python tools/latex-audit/run.py D:/projects/StrictLib/testdata/CC0_TEX target/latex-audit-verified-2026-10-18 --exe target/latex-audit-build/release/latex-audit.exe --pages 0 --timeout 90 --workers 2
python tools/latex-audit/summarize.py target/latex-audit-verified-2026-10-18 docs/history/audit-latex-2026-10-18
```

Each input runs in a separate process with a 64 MiB stack. `--pages 0` requests
all pages; other values restrict the prefix and are explicitly recorded.
The runner keeps import counters (including capped details), writer receipt,
DOCX, reopened XML, plain text, mathematical text, every page's SVG and the
first three PNGs. Rasterization loads the same bundled font programs used by
the renderer; it does not silently substitute arbitrary installed faces.

`stage=complete` is a pipeline smoke result, **not** fidelity acceptance.
The geometric check counts only text outside physical page bounds; it cannot
prove absence of overlap, correct glyphs, or mathematical equivalence. Stored
overflow examples are capped at 200 per case. Main-document XML statistics
exclude footnote/header parts. No external TeX engine, shell escape, corpus
scripts, network resource fetching or Word automation is used.

`math_clearance_candidates` separately exports renderer warnings prefixed
`render.math-clearance:`. These measure painted-ink bounds between semantic
neighbours: equation/matrix rows, display lines and fraction parts. A candidate
is an overlap, insufficient clearance or non-finite geometry. Conservative
glyph/vector rectangles are used; this is not a pixel-level collision oracle
and does not test every pair of objects on a page. Import regressions also
check clearance between distinct display paragraphs after DOCX roundtrip.

The nine audited defects now have active regression tests (no `ignore`):

```powershell
$env:TEMP='D:/projects/StrictLib-mufoed/target/test-temp'
$env:TMP=$env:TEMP
cargo +1.92.0 test -p strict-ooxml-latex --offline
cargo +1.92.0 test -p strict-ooxml-latex --offline --test cc0_regressions
```

The initial nine failures remain recorded in the dated audit logs; the current
regressions and additional controls pass. See the dated report and FIXES.md for the baseline,
confirmed defects, retained evidence and limits of visual verification.

Rasterize any saved page using the same bundled font programs:

```powershell
target/latex-audit-build/release/latex-audit.exe --raster INPUT.svg OUTPUT.png
```
