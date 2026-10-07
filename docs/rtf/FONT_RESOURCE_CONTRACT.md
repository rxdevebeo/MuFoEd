# RC02: explicit font assets and resource consumers

Current slice: 2026-10-07, based on `b6608f7`. RC02 remains InProgress.

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

## Renderer admission

`RenderOptions::font_assets(Arc<FontAssetRegistry>)` is optional; its default is
None. Layout resolves the original selected WML script-font slot before bundled
family substitution. Placed text retains the immutable registry through its run,
so the exact resource remains available after options and caller owners are dropped.
The metrics adapter also supports existing provider-based measurements. Admitted
resources must have usable vertical metrics; absent bindings keep the established
bundled resolution path. No style binding is fabricated by the registry.

SVG embeds the selected program bytes. CSS family names encode program hash,
face index and coordinate bits; caller family metadata is never inserted into CSS.
Scalar and shaped positions use the registered resource. Explicit variation settings
use escaped axis tags; optical sizing and synthetic font styles are disabled.
SVG currently rejects collection programs explicitly, including index zero in a TTC;
it does not ask a browser to guess the intended collection face.

PDF face keys contain an owned family identifier and optional FaceInstanceKey;
legacy bundled keys have no explicit instance. The collector retains FontResource,
collects scalar/cluster CIDs against that resource, and subsets its exact program,
collection index and every variation coordinate. Font dictionaries, widths and
ToUnicode use that subset. Registered text spacing/compression follows the SVG
origin adjustments. Resource keys prevent programs and instances from merging even
when their declared resolved family metadata is identical.

This is renderer API admission, not CLI font discovery/configuration. Default CLI
corpus runs do not exercise positive CJK coverage. Missing glyph warnings are visible,
but ReportAndContinue/Reject and exact/substitute/unresolved runtime reports remain
pending. In particular, browser missing-glyph fallback is not full SVG/PDF equivalence.
Complex script support remains explicitly degraded; these fixtures use bundled
programs admitted under test aliases and do not establish CJK/Indic support.

## Validation

Ten registry/SVG public tests cover the preceding six admission/API witnesses plus
explicit document layout/delivery/lifetime, safe CSS names, unchanged empty-registry
output, script-slot missing glyph diagnostics, TTC rejection and bounded lookup.
PDF witnesses independently parse/extract output with lopdf: distinct programs and
variable weights sharing declared family metadata, ownership after options drop,
combining marks/ligatures, deterministic PDF bytes, unchanged empty-registry output,
and TTC face 1 geometry matching the same explicitly admitted single-face program.
Public RTF integration exercises Latin and East Asian slot admission in both backends;
the CJK fixture deliberately lacks outlines and requires a loss diagnostic.

Full SVG/PDF/public RTF API suites: 425 Pass in 52 groups, exit 0, with the existing
`maps_regular_postscript_names_to_the_same_bundled_faces` exclusion recorded.
Final SVG registry suite after optical-sizing change: 10 Pass. MSRV Rust 1.92 Clippy
passes with the unchanged baseline suppressions: manual_let_else,
items_after_statements, single_char_pattern, replace_box, while_let_loop,
unnecessary_sort_by, if_same_then_else. Remaining pedantic baseline warnings are
not zero-warning acceptance. Changed-file rustfmt and git diff --check are required.
Logs: `target/rtf-import/RC02-render-assets-{full-tests,svg-final,clippy-final}.log`.

Fresh default public CLI corpus: 100/100 text, integrity and resource Pass;
all 100 DOCX hashes match the preceding East Asian face receipt. Strict XSD/OPC
validate all 100 packages with no violations. Corpus manifest SHA-256 remains
`0e0567956a1590ea8e273ece693dcd2d2b8cc408494d20c1234193cd99786a4f`;
CLI SHA-256 is `87a75e5c7a61591a43c555d103ddbbe71eff1abb0662e83debadd4e0ea42bb7b`.
Receipts: `RC02-render-assets-corpus-current/results.json` and
`RC02-render-assets-{xsd-current,opc-current}.log`, under `target/rtf-import`.
Pagination/pixel evidence is recorded separately in the RC02 status ledger; matching
page counts do not establish visual acceptance.

## Next implementation

1. ReportAndContinue/Reject policy and original-request exact/substitute/unresolved
   diagnostics, including missing glyphs, shaping and embedding failures.
2. Licensed CJK/Indic assets and independent positive geometry/extraction witnesses.
3. Reviewed script/font support profile and full-corpus visual acceptance; CLI asset
   configuration must use the same bounded registry rather than system discovery.
RC02 closes only after those requirements pass.


Fresh default pagination: 62/100 Pass, 38 Fail. All 100 actual/reference PDF hashes
and page correspondences match RC02-eastasia-face. The prior pixel receipt is
carried forward by exact PDF identity: 3/100 documents Pass, 97 Fail; 423 pages,
6 Pass and 417 Fail. The existing release comparator SHA-256 matches that receipt.
This is not a new raster run or visual acceptance. Equivalence evidence:
`target/rtf-import/RC02-render-assets-pixel-equivalence.json`; fresh pagination:
`target/rtf-import/RC02-render-assets-visual-verified/results.json`.
