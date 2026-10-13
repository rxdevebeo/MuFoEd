# Vendored crates

Permissively licensed crates we carry small patches for (AUD-88). Each directory
keeps the upstream licence files unchanged. Every patch is marked
`PrintCraft patch:` (or `PrintCraft / MuFoEd patch:`) in the source. Patches come
from [storytold/printcraft](https://github.com/storytold/printcraft) `vendor/`
(MIT/Apache-2.0), plus a MuFoEd extension that rejects absurd image sizes
*before* decode. Covered by `strict-ooxml-pdf/tests/hostile.rs` `mod raster`.
Remove the vendored copy once upstream releases the fix.

| Crate | Version | Licence | Patches | Test |
|---|---|---|---|---|
| hayro-interpret | 0.7.0 | Apache-2.0 OR MIT | `MAX_PAINT_NESTING` (tiling + Type 3); `MAX_CID` on `/W`/`/W2`; `ImageXObject::new` rejects width×height over 2^28 | `hostile::raster::{self_referencing_*, huge_cid_*, absurd_image_*}` |
| hayro | 0.7.1 | Apache-2.0 OR MIT | `MAX_IMAGE_PIXELS` skip in `draw_image` (ImageData and Device paths) | `hostile::raster::absurd_image_dimensions_are_skipped` |
| hayro-syntax | 0.7.2 | Apache-2.0 OR MIT | page-tree cycle guard; JBIG2 pixel budget (hayro#1259); `MAX_OBJECT_NESTING` on Dict/Array skip/read (AUD-96) | `hostile::raster::{page_tree_kids_cycle_is_handled, absurd_jbig2_*, deep_literal_dict_nesting_is_refused}` |

Wired via `[patch.crates-io]` in the workspace `Cargo.toml`.

### Behavioural patches (not security)

These come with the PrintCraft copy and change rendering, not limits. A rebase
onto a new upstream must carry them over too; each site is marked
`PrintCraft patch:` in the source.

| Crate | File | Patch |
|---|---|---|
| hayro-interpret | `src/soft_mask.rs` | a soft-mask group without `/CS` falls back to DeviceRGB instead of dropping the whole mask (ISO 32000-2 §11.6.5.2) |
| hayro-interpret | `src/interpret/mod.rs` | `NoView` annotation flag honoured; `hide_comments` keeps `/Widget` and `/Link`; `/AP /N` as a dictionary of appearance states selected by `/AS` |
| hayro-interpret | `src/ocg.rs`, `src/context.rs` | viewer overrides for optional-content groups (`ocg_overrides`) |
| hayro-interpret | `src/context.rs`, `src/pattern.rs`, `src/font/type3.rs` | `MAX_PAINT_NESTING` definition and its tiling/Type 3 call sites (listed above) |
| hayro-syntax | `src/byte_reader.rs` | `MAX_OBJECT_NESTING` counter used by `object/dict.rs` and `object/array.rs` (listed above) |

To list every patch site: `grep -rn "PrintCraft" vendor --include=*.rs`.

### Publishing

`[patch.crates-io]` applies only inside this workspace. A crate published to
crates.io that depends on `hayro` resolves the **unpatched** upstream release,
without the limits above. To fail closed, the vendored `hayro` exports a
`PATCHED_LIMITS` marker and `strict-ooxml-pdf/src/raster.rs` references it: a
`raster` build against upstream `hayro` does not compile. Default-feature builds
(no `raster`) do not depend on `hayro` and are unaffected. Remove the marker
reference together with the vendored copy once upstream ships the fixes.

### WMF / EMF previews (`emfsdk`)

`emfsdk` 0.2.0 (MIT OR Apache-2.0, [KaiserY/emfsdk](https://github.com/KaiserY/emfsdk))
rasterizes the WMF/EMF preview of an OLE object (a MathType equation) to its
laid-out frame (`strict-ooxml-render-svg/src/metafile.rs`). The crate ships no
licence files; its `Cargo.toml` states the licence. Every patch is marked
`StrictLib patch:` in `src/render.rs` (`grep -rn "StrictLib patch" vendor/emfsdk`).
Covered by `strict-ooxml-render-svg/tests/metafile_preview.rs`; each test fails
against the upstream release.

| Patch | Why |
|---|---|
| Font height and pen width are logical units, mapped to the canvas like coordinates (`EmfVectorState::draw_text`, `set_pen_pixel`) | Taken as device pixels, a MathType font (384 units at 2306/inch, ~32px) was drawn 384px tall on a 66x30 raster: the equation showed as a fragment of one glyph, or nothing, and a fraction bar as a solid block |
| `SetTextAlign` / `EMR_SETTEXTALIGN`: `TA_UPDATECP` (the run starts at the current position and moves it), `TA_TOP`/`TA_BASELINE`/`TA_BOTTOM`, `TA_LEFT`/`TA_CENTER`/`TA_RIGHT` | MathType places every run by `MoveTo` and writes (0, 0) in `ExtTextOut`; drawn at (0, 0) the text sat above the raster |
| The `ExtTextOut` `dx` array positions the glyphs (WMF and EMF, `ETO_PDY` pairs too) | The producer's spacing, not our fallback font's advances |
| The run's LOGFONT picks the face (family, weight, italic); a glyph the face lacks comes from a fallback chain; system fonts are scanned once per process | One sans-serif face for everything; a single missing glyph sent the whole run to the 5x7 bitmap font; every metafile rescanned the system fonts |
| The Symbol face's codes are mapped to Unicode (Adobe Symbol encoding; `U+F020..U+F0FF` in UTF-16 runs); other single-byte runs are decoded in their LOGFONT charset (Windows-125x) | `0xCE` drew `Î` instead of `∈` |
| Glyphs are filled with anti-aliased coverage (4 sub-scanlines, exact span overlap) | One sample at each pixel centre dropped a stroke thinner than a pixel - a minus sign - unless it crossed that centre |
| Faces are read with `skrifa` 0.44 and found with `fontdb` 0.24 instead of `ttf-parser` 0.25 / `fontdb` 0.23 | `ttf-parser` is unmaintained (RUSTSEC-2026-0192); `fontdb` 0.24 and the rest of the workspace already parse fonts with `skrifa` |

Known gaps: `MT Extra` codes (accent dots and the like) are drawn as their
Latin-1 characters; glyphs come from the system fonts, not the bundled ones, so
a raster differs between machines.

### DjVu import budgets

`djvu-bzz` and `djvu-jb2` 0.40.2 are vendored from MIT-licensed
[matyushkin/djvu-rs](https://github.com/matyushkin/djvu-rs), independently of
the PrintCraft patches above. Their original license notices are retained.
The `bzz_decode_with_limits`, `decode_with_limits` and
`decode_dict_with_limits` and `decode_indexed_with_limits` APIs enforce caller budgets before allocating blocks,
canvases and record tables. Existing public APIs remain available.
These patches are wired in both the root and separate fuzz workspaces;
`strict-ooxml-djvu/tests/read.rs` and `tests/image.rs` exercise their guards.
See `strict-ooxml-djvu/PROVENANCE.md` for derivation.
