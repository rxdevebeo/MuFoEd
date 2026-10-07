# RC02: explicit font assets and resource consumers

Current slice: 2026-10-07, based on `9a30a06`. RC02 remains InProgress.

## Resource contract

`FontResource` holds borrowed bundled bytes or explicit `Arc<[u8]>`. Its key is
program SHA-256, collection face index and sorted explicit variation coordinates.
The legacy Copy `ResolvedFace`/`FaceSource` APIs remain available. Family aliases
are separate from resource identity; different variable weights remain distinct.
Omitted defaults are not yet merged with explicit default coordinates.

`from_shared` validates font/index, duplicate axes, supported axes and design ranges.
`FaceVariation::new` rejects nonfinite values and canonicalizes negative zero.
`ResolvedFaceHandle` carries original request, declared resolved family, requested
style, mapping provenance and the resource. ExactFamily identifies the declared
family binding; it does not prove internal font naming, style authenticity, glyph
coverage or full script support. Those facts need independent asset witnesses.

## Immutable bounded registry

`FontAssetRegistry::from_assets(Vec<FontAsset>, FontRegistryLimits)` takes explicit
shared bytes and a typed manifest per family/style binding. Each manifest pins
program hash, face index, variations, declared family/style, source reference,
license identifier/reference and explicit EmbedAndSubset permission attestation.
The library does not fetch or legally verify those evidence references.

Complete budget/metadata/duplicate/permission preflight runs before any program
hashing/parsing. Construction is transactional; failure contains input asset index
and a specific reason. OS/2 flags are checked independently: missing metadata,
restricted embedding, no-subsetting, bitmap-only and unknown bits reject admission.
A permissive attestation cannot override restrictive font metadata.

| Budget | Default | Hard ceiling |
| --- | ---: | ---: |
| bindings | 64 | 1024 |
| bytes per binding | 32 MiB | 128 MiB |
| sum of binding bytes | 128 MiB | 1 GiB |
| bytes per source/license field | 4096 | 16384 |
| variation coordinates per binding | 32 | 64 |

Family fields are bounded to 256 UTF-8 bytes. Empty/control-containing metadata,
zero/oversized budgets and duplicate normalized family/style bindings are rejected.
Bytes are charged per binding, even if bindings share an Arc. Lookup trims and
normalizes family case, requires the exact bold/italic binding, performs no system
font discovery and returns None for an absent binding instead of synthesizing a
style or selecting a fallback. Manifest enumeration is independent of input order.

## Owned measurement and shaping

`FontResource::metrics`, `glyph_id` and `advance_em` read the exact selected program
and variation location. Missing glyphs/metrics return None; no character-class
advance or family substitution is invented by these operations.

`shape_resource(text, &ResolvedFaceHandle)` uses existing harfrust plus the shared
raw-glyph/cluster assembly. `ShapedText<F = ResolvedFace>` keeps legacy type usage
available and supports owned handles. Unicode cluster/x helpers accept either face
kind. Borrowed parser/shaper state is local to the operation; returned shaped text
keeps its resource alive. Owned parsed-face/run caching is a later optimization.
Missing `.notdef` glyphs return DegradedMissingGlyph; otherwise complex scripts retain
DegradedComplexScript. This does not establish independent CJK/Indic acceptance.

The legacy shaping caches still include program/instance identity. Existing PDF
collection/build still adapts the shared cached bundled resolver to FaceSource.
Registry assets are not yet wired into RenderOptions/placement/SVG/PDF. Therefore
these APIs enable explicit resource operations, not external-font document rendering.

## Validation

Six public API regressions cover registry order/lifetime, absence of implicit style
or fallback, owned metrics/shaping parity, weight identity, explicit missing CJK
glyphs, all budgets, preflight ordering, pinned hashes/index/axes/metadata/duplicates,
and OS/2 embedding restrictions overriding permission attestations.

Full SVG/PDF/public RTF API suites: 418 Pass in 52 groups, exit 0, with the existing
`maps_regular_postscript_names_to_the_same_bundled_faces` exclusion recorded.
Final registry tests after lint fixes: 6 Pass, exit 0.
MSRV Rust 1.92 Clippy: Pass with unchanged baseline suppressions:
manual_let_else, items_after_statements, single_char_pattern, replace_box,
while_let_loop, unnecessary_sort_by, if_same_then_else.
Changed-file rustfmt and git diff --check: Pass.
Logs: `target/rtf-import/RC02-registry-{full-tests,tests,clippy}.log`.

Corpus/XSD/OPC/pixels were not rerun for this API-only slice. Previous measurements
are not new receipts; a fresh corpus run is required after renderer admission.

## Next implementation

1. Optional registry in RenderOptions; preserve original font-slot request through
   placement and retain one resolved owned handle for measurement and delivery.
2. SVG font emission/positions and PDF collector/subsets migrate to resource keys,
   bytes, face index and all variation coordinates; no fallback back to static APIs.
3. ReportAndContinue/Reject policy and original-request exact/substitute/unresolved
   diagnostics, including missing glyphs and embedding failures.
4. Licensed CJK/Indic assets, independent geometry/extraction witnesses, determinism
   and fresh full-corpus gates. RC02 closes only after those requirements pass.
