# Stage 5C — OMML formulas: report

**Order:** `STAGE-5C-TASK.md` (ZAKAZ-STAGE-5C)
**Echelon:** 5C of Stage 5 (formulas), third and last echelon of Stage 5
**Date:** 2026-09-29
**Status:** **implemented, not accepted** — see §7

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
| S5C.7 | `coverage/stage5-scenarios.toml` | 40 `math.*` entries; coverage 92.4 % → **94.3 %** | done |
| S5C.8 | `xtool gen-docx --stage5c`, `tests/strict/strict-stage5c.docx`, `refs/` | fixture + WPS references | done, gate open |
| S5C.9 | this report | — | partial |

## 2. Customer decisions (§9)

1. **Font** — STIX Two Math bundled **and** a deterministic fallback: every
   *stretchy* construct (delimiters, radical sign) is drawn as a scaled vector
   path, so the structure is right even for a character the face lacks.
2. **Subset** — all of §3.1.
3. **`m:oMathPara`** — full: `m:oMathParaPr/m:jc` with left/center/centerGroup/
   right, its own line, extra leading.
4. **Export** — MathML projection added (`math_expression_to_mathml`,
   `math_paragraph_to_mathml`); it doubles as the independent structural view.
5. **Fixture** — one document, every construct inline and as a display formula.

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

## 4. Layout

Deterministic TeX-style two-pass layout; every constant is a fraction of the
current font size, so a formula scales exactly with its paragraph.

| Quantity | Value |
|---|---|
| math axis | 0.25 em |
| script level | 0.62 em, shifts 0.38 / 0.18 em |
| rule thickness | 0.045 em (min 0.6 px) |
| delimiter size | content + 0.28 em, width 0.26 × height, `grow` honoured |
| formula line | 1.30 × natural line height, capped at 1.60 × |
| display formula | 10 pt before and after |

## 5. Fonts

`STIXTwoMath-Regular.otf` (SIL OFL 1.1) from CTAN `stix2-otf`,
`sha256 95bc2729e41faf93…`. `map_family("Cambria Math") → "STIX Two Math"`. One
upright face serves every style, so `m:sty` reaches the SVG as `font-style` /
`font-weight` only. `FontProvider::has_glyph` lets the layout choose between a
glyph and the vector fallback.

## 6. Verification

| Check | Result |
|---|---|
| `cargo fmt --all -- --check` | pass |
| `cargo clippy --workspace --all-targets --all-features -- -D warnings` | pass |
| `cargo test --workspace --all-features` | pass (432 tests) |
| `cargo doc --workspace --no-deps` | pass |
| `cargo deny check` | pass |
| `xtool coverage --file coverage/stage5-scenarios.toml --min 85` | **94.3 %** |
| SSIM `strict-text` / `strict-text-grid` / `strict-stage5` / `strict-stage5b` | 0.9768 / 0.9709 / 0.9795 / 0.9640 — unchanged |
| SSIM `strict-stage5c` | **0.945 — below the 0.95 criterion** |

New tests: 16 parsing (`wml/tests/math.rs`), 18 rendering and MathML
(`render-svg/tests/math.rs`), 4 independent-oracle checks
(`wml/tests/stage5c_fixture_oracle.rs`, `zip` + `roxmltree`, cross-checking the
constructs, the namespaces and the concatenated `m:t` text against our model).

## 7. Why 5C is not accepted

**Criterion 2 fails.** Worst-page SSIM on `strict-stage5c` is 0.945 against the
required 0.95, and the row-ink profile correlation is 0.400.

The cause is a layout defect, not a tolerance question: a multi-row `m:eqArr`
inside `m:d` reports a depth of ~178 px for two 11 px rows, so the surrounding
`{` is drawn roughly 150 px too tall and dominates the page. The font metrics
involved are correct (STIX ascent 0.762 em, descent 0.238 em), so the fault is
in the grid size bookkeeping in `math::layout::Grid::place` /
`layout_equation_array`. The sign error and a double-count were fixed; the
remaining cause was not isolated before the echelon was checkpointed.

The document is listed in `PENDING_DOCS` in `tests/ssim.rs`: its page count and
"not blank" checks **are** enforced, and its structural result is printed with
`not enforced` so the shortfall is visible on every run. No threshold was
loosened to make it pass.

Separately, `strict-stage5c` page 1 is excluded from comparison outright: WPS
`12.1.0.28485` lays consecutive `m:oMathPara` **on top of each other** instead
of stacking them, so that reference records a producer defect. Our layout stacks
them, which is the correct behaviour.

## 8. Open items

1. Fix the `m:eqArr` depth computation and re-gate `strict-stage5c` at
   SSIM ≥ 0.95 with the structural invariant.
2. `docs/stage-5c-report.md` §5.3/§5.4 numbers should be re-measured after
   the fix; `MATH_LINE_SPACING` / `MAX_MATH_LINE_GROWTH` were tuned against a
   reference that contained the defect.
3. `m:vertJc` is parsed and reported but its effect on over/under-brace
   alignment is not reproduced — the coverage map records this as `partial`; the
   fixture deliberately omits it because the producer's own output for that
   combination is not a usable target.
4. ADR-0004/0006 updates, the 100-page performance run (§8 of the order) and
   `STAGE-5C-ACCEPTANCE.md` are not written yet.
5. Independent confirmation of the 5C coverage map is still open, as for 5A
   (A-1/A-2) and 5B (A-5B-2).
