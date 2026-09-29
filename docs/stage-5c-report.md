# Stage 5C — OMML formulas: report

**Order:** `STAGE-5C-TASK.md` (ZAKAZ-STAGE-5C)
**Rework:** `STAGE-5C-REWORK-1.md` (ZAKAZ-STAGE-5C-1)
**Echelon:** 5C of Stage 5 (formulas), third and last echelon of Stage 5
**Date:** 2026-09-29
**Status:** implemented; **accepted with reservations** — see
`STAGE-5C-ACCEPTANCE.md` and §9

---

## 1. What was built

OMML (`http://purl.oclc.org/ooxml/officeDocument/math`) is a native part of
ISO/IEC 29500-1, so a Strict-first implementation is exact — unlike the
`wps`/`wpg` extensions of echelon 5B. All twenty constructs of §3.1 are parsed
into a typed model, laid out, and drawn to SVG.

| Order item | Artefact | State |
|---|---|---|
| S5C.1 | `wml/src/model/math.rs` | 20 constructs, locations, interning via the existing parser | done |
| S5C.2 | `wml/src/parse/math.rs` | Strict namespace only, node/depth budgets, `Partial` reporting | done |
| S5C.3 | `render-svg/src/math/layout.rs` | axis, scripts, fractions, radicals, delimiters | done |
| S5C.4 | same | matrices, equation arrays, accents, n-ary, `limLoc` | done |
| S5C.5 | `layout/paragraph.rs`, `paginate.rs` | inline on the baseline, display as its own block, `--no-math` | done |
| S5C.6 | `assets/fonts/stix/`, `font/{builtin,family}.rs` | STIX Two Math (OFL) + vector fallback | done |
| S5C.7 | `coverage/stage5-scenarios.toml` | 43 `math.*` entries; coverage **94.4 %** | done |
| S5C.8 | `xtool gen-docx --stage5c`, `tests/strict/strict-stage5c.docx`, `refs/` | fixture + WPS references | done, **gated** |
| S5C.9 | this report, ADR-0004/0006, `STAGE-5C-ACCEPTANCE.md` | — | done |

## 2. Customer decisions (§9)

1. **Font** — STIX Two Math bundled **and** a deterministic fallback: every
   *stretchy* construct (delimiters, radical sign, large operators) is drawn as
   a scaled vector path, so the structure is right even for a character the face
   lacks. See §5 for the metric decision this rework settled.
2. **Subset** — all of §3.1.
3. **`m:oMathPara`** — full: `m:oMathParaPr/m:jc` with left/center/centerGroup/
   right, its own line.
4. **Export** — MathML projection (`math_expression_to_mathml`,
   `math_paragraph_to_mathml`); it doubles as the independent structural view.
5. **Fixture** — one generated document (every construct inline and as a display
   formula) **plus** five real hand-built Strict packages (`05`–`10`) added by
   the rework order, so the fidelity work is measured against a producer we did
   not write.

## 3. Parsing

`m:oMath` / `m:oMathPara` are paragraph content, so they arrive through the
inline path and become `Inline::Math` / `Inline::MathParagraph` instead of
`Opaque`. Every node carries a `SourceLocation`. Per-formula budgets — 4096
nodes, 64 levels of nesting — are new `LimitKind` variants; a violation is
`LimitExceeded`, never a panic.

Three defects were found while testing the parser, all of them silent:

* the XML reader emits a **synthetic `EndElement` for every self-closing tag**,
  so a property loop that stopped at the first `EndElement` ended on the
  property element instead of on its container — every `m:val` was lost;
* `m:num`/`m:den`/`m:fName` **are** `CT_OMathArg`; wrapping them in another
  `m:e` (as the first draft of the fixture did) desynchronised the parse;
* `m:*Pr` was being read *after* its start tag had already been consumed.

### 3.1 `w:docDefaults` (C1e)

`w:docDefaults` was parsed as `Partial` and skipped, so the document-wide
defaults of `styles.xml` never reached the cascade. Every real Strict package
in the repro corpus sets its font and its `w:spacing w:line`/`w:after` there,
which is why the whole corpus laid out at the wrong line height. `StyleTable`
now carries a `DocDefaults` (`w:pPrDefault/w:pPr` + `w:rPrDefault/w:rPr`) and
`compute_paragraph` seeds the cascade from it, before any style. The mechanism
moved from `partial` to fully modelled; `wml/tests/styles_settings.rs` asserts
it.

## 4. Layout

Deterministic TeX-style two-pass layout; every constant is a fraction of the
current font size, so a formula scales exactly with its paragraph. The numbers
below were **re-measured** against the WPS references (`STAGE-5C-REWORK-1` C1);
the ones the previous report carried had been tuned against a reference that
contained our own defect.

| Quantity | Value | Source |
|---|---|---|
| math axis | 0.25 em | Word default |
| script level | 0.62 em, shifts 0.38 / 0.18 em | Word professional style |
| rule thickness | 0.045 em (min 0.6 px) | — |
| formula row (nominal) | 1.164 em — Cambria Math `hhea` (1901 + 483 / 2048) | measured: 17.0 px at 11 pt on both references |
| fraction parts | ±0.40 em from the baseline, rule on the axis | measured: 1.46 em total on `05` |
| large operator (display) | 1.98 em × 0.66 for `∑`, 2.18 em × 0.28 for `∫` | measured on the `06` reference |
| delimiter | content + 0.28 em, width 0.26 × height, `grow` honoured | — |
| formula line | natural height, grown by the formula's own box, capped at 1.60 × | both references measure the natural height |
| display formula | own block; **no** `w:spacing` added around it | `10`, `06`, `strict-stage5c` |

### 4.1 The five defects the rework closed (C1)

1. **`m:rSp`/`m:cSp` units.** ISO/IEC 29500-1 §22.1.2.79 makes them
   `ST_UnsignedTwipsMeasure`; the layout read them as points, so
   `m:rSp w:val="120"` became a 160 px row gap. A two-row `m:eqArr` was 175 px
   tall instead of 35 px. Regression test:
   `render-svg/tests/math.rs::an_equation_array_row_spacing_is_twips`.
2. **Empty lines around a display formula.** The `m:oMathPara` handler emitted
   the pending line even when empty, before *and* after its block, and the
   paragraph added an invented 10 pt on both sides. Every display formula
   therefore pushed the following text down a full line. Regression test:
   `render-svg/tests/ssim.rs` (`05`/`06`/`10` are SSIM-gated).
3. **`w:docGrid` without `w:type`.** ISO/IEC 29500-1 §17.6.6: `w:type="default"`
   — also the value when the attribute is absent — is *no* document grid;
   `linePitch` then only fixes the lines per page. The renderer snapped anyway,
   which turned a 19.3 px line into 24 px on every repro package. Regression
   test: `render-svg/tests/math.rs::a_default_document_grid_does_not_snap_line_heights`.
4. **`m:f/m:type="lin"`.** The linear fraction fell through to the bar case and
   grew the line instead of staying inline. It now keeps both parts on the line
   with a solidus between them, bounded by one line of the math font.
5. **Large operators.** `m:grow` was implemented as a horizontal widening only.
   Word promotes `∑`/`∫`/`∏`… to a *display* cut of the math font inside
   `m:oMathPara` and keeps the text cut inline; the operator is now stretched to
   a tabulated display size (per family) and centred on the math axis, and the
   limits clear its ink rather than its baseline.

### 4.2 What is still off, measured

`strict-stage5c` page 2 — three display blocks, the worst case in the corpus —
accumulates **11 px** of vertical drift over 135 px of content (1.0 % of the
page height), and its horizontal ink centroid is 23.6 px off. Both are the
residual of the block *box* of a stretched operator and of the ink distribution
inside it (a stroked path carries less ink than WPS's filled glyph). The other
five gated fixtures are within 11 px on both axes and mostly at 0–1 px of
shift. `STAGE5C_LIMITS` records the bounds and why they are what they are; the
pre-rework defect this replaced was 400 px on the same page.

## 5. Fonts

`STIXTwoMath-Regular.otf` (SIL OFL 1.1) from CTAN `stix2-otf`,
`sha256 95bc2729e41faf93…`. `map_family("Cambria Math") → "STIX Two Math"`. One
upright face serves every style, so `m:sty` reaches the SVG as `font-style` /
`font-weight` only. `FontProvider::has_glyph` lets the layout choose between a
glyph and the vector fallback.

### 5.1 The math-font decision (C3)

The order offered (a) a metric-compatible Cambria Math substitute or (b) a
structural gate with an agreed lower SSIM. **We took (a)'s spirit and (b)'s
harness, and recorded it in ADR-0006:**

* No OFL face is metric-compatible with Cambria Math. `ATTRIBUTION.md` already
  said so for STIX Two Math, and the acceptance measured the ceiling it
  imposes (`05` 0.9484, `06` 0.9406, `10` 0.9349 **before** any layout fix).
  Searching for a better face was therefore not going to buy the threshold.
* What *is* reproducible is the **block geometry**: STIX is a glyph substitute,
  so the layout now uses the **nominal** font's line metrics (Cambria Math
  `hhea`) for the row grid, the fraction box and the display-operator sizes —
  the same substitution principle `map_family` already applies when it puts
  Carlito's advances behind Calibri. The row pitch went from 18.3 px to the
  reference's 17.0 px, and the large operators to their measured sizes.
* The **0.95 threshold is unchanged**. No document's gate was loosened to make
  it pass; the only bounds that moved are the structural ones of
  `STAGE5C_LIMITS`, which are written out with their justification in
  `tests/ssim.rs` and are still backed by negative controls
  (`structural_check_rejects_blank_and_shifted_stage5c_pages`).
* The residual difference is glyph shapes and weights, which no SSIM threshold
  can absorb. That is recorded as a reservation, not as a solved problem.

### 5.2 Liberation families (D1)

`map_family` did not know `Liberation Sans/Serif/Mono`, so a LibreOffice Strict
package fell through to the rasterizer's fallback: the text came out **grey**
(ink fraction `< 0.5` = 0.0000 against the reference's 0.0061) with different
metrics. The three families map to the bundled `Arimo`/`Tinos`/`Cousine`, which
are the same metric-compatible designs under the OFL names. After the fix the
sample renders black text (ink 0.0072, SSIM 0.9560). Unit test:
`font::family::tests::maps_the_libreoffice_families_to_their_bundled_twins`.

## 6. Settings/theme classification (D2)

Real Word and LibreOffice Strict files were reported as *blocked* on elements
that cannot change a rendered page: `w:characterSpacingControl`,
`w:clrSchemeMapping`, `w:rsids`, `a:objectDefaults`,
`a:extraClrSchemeList`, the `w14`/`w15` identity settings and the editing /
custom-XML compatibility settings. `record_foreign` now routes a table of such
elements through `harmless_element`, which returns `ignored` (with the reason
kept in the Feature Report) or `partial`. `check` exits `0` on every package of
the repro corpus; `w:mathPr`/`m:mathPr` stay `partial` because the document math
defaults genuinely are not applied.

## 7. DrawingML shapes (D3) and the page invariant (D4)

`07` rendered every `wps:wsp` as a grey placeholder rectangle: the inline
image path ran first and claimed any inline drawing that declared an extent,
including shapes. `layout_inline_image` now yields to `graphics::inline_items`
for a shape or group, and an inline drawing is placed **in the text line** with
its bottom on the baseline (as Word places `wp:inline`) instead of forcing a
block of its own. `07` went from SSIM 0.9065 / ink 0.0069 to **0.9568 / 0.0168**
against a reference ink of 0.0164.

`09` spilled onto a second page. It is a real-Strict package with a DrawingML
**chart part**, which is Stage 6 scope; the page-count invariant is what is
meaningful for it, and it now holds (1 page). `strict-profile` is page-count
only for the same reason and has been since Stage 4.

## 8. Verification

| Check | Result |
|---|---|
| `cargo fmt --all -- --check` | pass |
| `cargo clippy --workspace --all-targets --all-features -- -D warnings` | pass |
| `cargo test --workspace --all-features` | pass (443 tests) |
| `cargo doc --workspace --no-deps` | pass |
| `cargo deny check` | pass |
| `xtool coverage --file coverage/stage5-scenarios.toml --min 85` | **94.4 %** |
| `xtool coverage --file coverage/wml-elements.toml --min 90` | pass |
| line coverage core / wml / report / render-svg (≥ 80) | 87.97 / 85.08 / 99.83 / 84.94 % |
| SSIM `strict-text` / `-grid` / `strict-stage5` / `strict-stage5b` | 0.9768 / 0.9709 / 0.9795 / 0.9640 — **unchanged** |
| SSIM `strict-stage5c` (both pages gated) | 0.9533 / 0.9761 |
| SSIM `05` / `06` / `07` / `10` | 0.9836 / 0.9746 / 0.9568 / 0.9722 |
| perf: 110 pages of dense inline OMML (3600 paragraphs, every construct) | **0.29 s** (limit 5 s) |
| perf: 100 text paragraphs | 0.13 s |

New tests this rework: `render-svg/tests/math.rs` (twips regression, doc-grid
regression), `render-svg/tests/ssim.rs` (5C negative controls),
`strict-ooxml/tests/stage5c_corpus.rs` (end-to-end over the five repro
packages: parse, report, page count, formula ink, `check`), and
`wml/tests/stage5c_fixture_oracle.rs` (the independent `zip` + `roxmltree`
oracle now also covers `05`/`06`/`10`).

### 8.1 A coverage-gate defect found on the way

`cargo llvm-cov -p strict-ooxml-wml` reported 79.80 % lines at the 5C commit
(below the CI threshold of 80) and 73.8 % after the rework — from a 217-line
change. The cause is not the change: with the default 16 codegen units the
public entry points (`parse_document` and friends) are emitted once per unit,
and `llvm-cov` merges by source line, so a line covered in one copy and absent
in another counts as uncovered. The number therefore moved with code layout
rather than with test coverage. `[profile.dev] codegen-units = 1` in the
workspace root makes the measurement deterministic; with it the crate measures
**85.08 %** lines. The other three crates are unchanged and pass.

## 9. Reservations

1. **Glyph-level fidelity is font-bound.** STIX Two Math is not
   metric-compatible with Cambria Math and no OFL face is; the block geometry
   now follows the nominal font, but glyph outlines, weights and widths still
   differ. This is the residual behind `06` (0.9746) and `10` (0.9722), both
   comfortably above the 0.95 criterion.
2. **`strict-stage5c` page 2 drifts 11 px** and its ink centroid 23.6 px across
   three stacked display blocks (§4.2). Enforced with a documented bound.
3. **Charts are not rasterized** (Stage 6): `09-strict-math-drawing-chart` and
   `strict-profile` are page-count only.
4. **`m:vertJc`** is parsed and reported but its effect on over/under-brace
   alignment is not reproduced — the coverage map records this as `partial`.
5. **Independent confirmation of the 5C coverage map** is still open, as for 5A
   (A-1/A-2) and 5B (A-5B-2).
