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


## RC02 cluster ActualText extraction (2026-10-07)

Multi-glyph shaped clusters now carry `/Span` marked content with UTF-16
`ActualText` for their complete logical Unicode span. The same CIDs, embedded
program, glyph origins and offsets are painted; single-glyph clusters retain their
ToUnicode path. Only nonempty clusters without missing glyphs receive replacement
text. This avoids fabricating an empty ToUnicode destination for continuation CIDs.

Independent MuPDF 1.26.7 (PyMuPDF 1.26.4) extracts the pinned Devanagari witness
exactly, without replacement characters; CJK also passes. The Rust regression
independently parses UTF-16 content properties and verifies balanced BDC/EMC around
all glyphs in each multi-glyph cluster. lopdf 0.45.0 ignores ActualText in its text
extractor: raw ToUnicode extraction still adds U+FFFD for continuation CIDs. That
reader limitation remains recorded; universal reader compatibility is not claimed.

Reproduce: set `STRICTLIB_FONT_WITNESS_OUTPUT` to a witness directory, run the PDF
test `licensed_programs_embed_with_cjk_unicode_and_explicit_indic_degradation`,
then run `python xtool/rtf/font_extraction_witness.py --pdfs DIRECTORY --receipt FILE`
with PyMuPDF 1.26.4 available. Missing inputs or extraction differences fail the gate.
Receipt: `target/rtf-import/RC02-actualtext-extraction.json`; full tests and Clippy:
`RC02-actualtext-{full-tests,clippy}.log`. 436 tests in 53 groups pass with the
previous alias-test exclusion; MSRV Clippy passes with existing suppressions and
baseline warnings. Corpus/XSD/OPC/pagination/pixels were not rerun in this focused
extraction slice; previous receipts remain historical and PDF byte identity is not
claimed after adding marked content.

Complex-script status and strict refusal remain unchanged: a successful narrow
extraction witness does not close Indic visual/placement acceptance. Next: review
script/font support profiles, broaden independent complex-script geometry/visual
witnesses, configure CLI assets through the bounded registry, and pass visual corpus.
RC02 remains InProgress.


## RC02 bounded CLI font assets (2026-10-08)

`render` and `to-pdf` take `--fonts <manifest.json>` (schema
`strictlib.font-assets.v1`). Each entry pins one exact family/style binding by
`sha256` and names a `path`; optional `face_index`/`variations` and
provenance/license metadata are carried through, and `embedding` must be
`embed-and-subset`. Only the named files are read (no system discovery), bounded by
`FontRegistryLimits::default()` before parsing. Admission reuses
`FontAssetRegistry`; `write` rejects the flag. `strict-ooxml` re-exports the asset
types under `svg`, and the registry flows through `RenderOptions::font_assets`, so
SVG and PDF embed the exact program bytes of the API path.

Four loader unit tests cover admission plus hash/permission/malformed-sha/schema
rejection and check the hardcoded witness hashes against
`fonts/witnesses/manifest.json`. One process-level test imports an RTF East Asian
slot, embeds the pinned CJK witness (a `StrictAsset-` family in the SVG) and
refuses a wrong hash with exit 2. `cargo +1.92.0 clippy --workspace
--all-targets --all-features -- -D warnings` passes; the merge also fixed
RTF-added `indexing_slicing`/`if_same_then_else`/`items_after_statements` spots
that the newly merged crate-level denies exposed. Corpus/visual/pixel gates were
not rerun; this slice adds configuration, not a new positive-corpus measurement.


## RC02 reviewed support profile and extraction decision (2026-10-08)

[profile.md](profile.md) is the reviewed script/font support profile: Latin/
Cyrillic/Greek supported through bundled faces and aliases; East Asian supported
for the finite pinned CJK witness; Devanagari degraded (geometry/embedding and
ActualText extraction pass, strict rendering refuses on `font.script.degraded`);
other complex scripts unsupported; variable/collection faces supported for
delivery (SVG rejects collections); missing requests degraded by default and
rejected under the strict policy. Each row states its independent evidence and its
explicit limits.

Extraction decision: multi-glyph clusters emit spec-compliant UTF-16
`/ActualText`; ActualText-aware readers (MuPDF/Adobe/PDFium) extract exactly, while
readers that ignore ActualText (the `lopdf` helper used as an independent oracle)
fall back to per-CID `ToUnicode` and show replacement characters. That is recorded
as a documented reader limitation, not an open renderer defect.

The two acceptance items remain **Blocked**: full complex-script placement/visual
acceptance is owned by RC03 shaping / RC04 metrics, and the full corpus
pagination/pixel gate needs poppler (`pdfinfo`/`pdftoppm`) plus the pinned
comparator and the WPS reference PDFs (present in the sibling `rtf-import`
worktree). Neither is a font-resolution defect. `STATUS.json` records the concrete
blockers.
