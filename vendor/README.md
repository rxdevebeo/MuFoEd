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
