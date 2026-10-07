# RC02 registry and renderer tests

Contract/limits/validation: [FONT_RESOURCE_CONTRACT.md](../../FONT_RESOURCE_CONTRACT.md).

## Earlier API-only slice

- Six `tests/font_assets.rs` public API regressions verify exact style lookup, deterministic
  enumeration, shared lifetime, owned metrics/shaping parity, budgets, pinned hashes,
  metadata/index/variation rejection and OS/2 flags overriding permission attestations.
- Missing CJK glyphs remain explicit; this negative witness is not CJK support acceptance.
- Earlier resource/shape-cache regressions remain in the full suite.
- Full SVG/PDF/RTF suites: 418 Pass with the existing single alias-test exclusion.
- Final registry suite: 6 Pass. MSRV Clippy retains only the recorded baseline suppressions.
- Renderer admission and fresh corpus/XSD/OPC/pixel receipts remain pending.


## Renderer admission slice

- Ten registry/SVG tests cover layout/embedded identity, retained ownership, escaped
  CSS instance settings, unchanged default output, missing script glyphs and TTC rejection.
- PDF independent extraction/parse witnesses cover resource/weight collisions,
  collection index 1, combining marks, deterministic output and ownership after options drop.
- RTF public integration resolves both Latin and East Asian aliases through the same registry.
- 425 full-suite Pass (52 groups, same baseline alias exclusion); final SVG 10 Pass.
- Clippy passes on MSRV with previously recorded baseline suppressions; formatting
  and diff checks are recorded in STATUS.json.
- Default corpus: 100 text/integrity/resource Pass; Strict XSD and OPC: 100, zero violations.
- These default CLI measurements and deliberately missing CJK fixtures do not establish
  positive CJK/Indic or full visual acceptance.

Fresh pagination is 62/100 Pass; prior pixels remain 3/100 Pass, carried forward
by exact equality of all actual/reference PDF hashes and page correspondence.
No fresh raster run is claimed; see pixel equivalence receipt in STATUS.json.


## RC02 failure policy and licensed witnesses (2026-10-07)

`RenderOptions::font_failure_policy` explicitly enables common original-request
font diagnostics. `ReportAndContinue` reports unknown requests, missing glyphs,
shaping failures, degraded scripts, unsupported SVG delivery and PDF embedding
failures. `Reject` refuses placement/backend output on the first such failure,
before deduplication or truncation. Known aliases/substitutes remain informational.
Registered runs always receive reports. Default bundled-only continuation preserves
legacy output/diagnostics; call the policy builder or set `font_diagnostics` to
inspect those runs too. `Page::font_report` and `PdfReport::font_report()` expose
program/index/coordinate identity, requested/resolved family and scalar when known.

Details default to 256 entries (hard ceiling 4096), each metadata/detail excerpt
is bounded to 512 UTF-8 bytes. Zero detail capacity keeps failure occurrence counts
and truncation state; `is_clean()` cannot turn true by dropping details. Counts
refer to failures observed in placed fragments, not distinct source characters.
Admission errors always remain errors. SVG cannot deliver TTC collections: continue
omits their text and records `font.delivery.unsupported`; strict refuses output.
PDF can subset the selected collection face; subset failures retain original request.

Renamed finite-coverage OFL derivatives of pinned Noto CJK SC and Noto Sans
Devanagari are checked in under `strict-ooxml-render-svg/fonts/witnesses`, with
copyright, license and source/fixture hashes in `manifest.json`. They are test
programs, not automatic fallbacks or general-purpose script coverage. Regenerate
with `xtool/rtf/font_witnesses.py`, fontTools 4.60.1 and uharfbuzz 0.56.3; provide
the pinned source programs via `--source` and the fixture directory via `--output`.
The independent native HarfBuzz 14.6.0 oracle verifies original/subset glyph names
and unchanged geometry; Rust witnesses compare glyph IDs, UTF-8 clusters, advances,
offsets and origins against saved TSV data. HarfBuzz API use is independent of
harfrust in the renderer.

CJK strict SVG/PDF and imported RTF East Asian slot witnesses pass; the PDF program
is embedded and independent lopdf extraction preserves the tested Unicode text.
Devanagari glyph coverage and geometry pass, and its TTF program embeds, but
continuation CIDs currently produce U+FFFD during lopdf text extraction. Its common
`font.script.degraded` diagnostic remains, and strict rendering refuses it. Closing
that ToUnicode/continuation gap and full Indic placement/visual acceptance remains
required; these narrow witnesses do not establish full CJK/Indic support.

Validation: 436 tests pass in 53 groups with the previously recorded
`maps_regular_postscript_names_to_the_same_bundled_faces` baseline exclusion.
MSRV Clippy passes with existing baseline suppressions and warnings; new witness
warnings were fixed. Receipts: `target/rtf-import/RC02-font-policy-full-tests.log`
and `RC02-font-policy-clippy-final.log`. Fresh default corpus: 100/100 text,
integrity and resource Pass in `RC02-font-policy-corpus/results.json`.
The historical pixel receipt remains historical, not a new raster run.

Next: continuation-CID Unicode extraction, reviewed script/font support profile,
CLI configuration using the bounded registry, and full-corpus visual acceptance.
RC02 remains InProgress.
