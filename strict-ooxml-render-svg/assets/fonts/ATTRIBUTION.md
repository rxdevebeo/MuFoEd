# Bundled font assets — attribution and licenses

These fonts are bundled **only** so the renderer (metrics + SVG rasterization)
produces deterministic, Word-comparable output without depending on system
fonts (ADR-0006). They are **metric-compatible** substitutes for the
proprietary fonts that Word documents request; the proprietary originals
(Calibri, Cambria, Arial, Times New Roman, Courier New) are **not** bundled.

No font was modified; the OFL license text is included alongside each family.

## Files

| Family (bundled) | Metric-compatible with | License | Files |
|---|---|---|---|
| Carlito | Calibri | SIL Open Font License 1.1 (`carlito/OFL.txt`) | `carlito/Carlito-{Regular,Bold,Italic,BoldItalic}.ttf` |
| Caladea | Cambria | SIL Open Font License 1.1 (`caladea/OFL.txt`) | `caladea/Caladea-{Regular,Bold,Italic,BoldItalic}.ttf` |

## Provenance

Downloaded from the **google/fonts** repository (github.com/google/fonts):

| Path | Pinned commit |
|---|---|
| `ofl/carlito` | `3dd78844021e948ceb633d1dcee3f7885561b5d9` |
| `ofl/caladea` | `13c010f766052c13fa9cc8c656008cee55fb4775` |

| File | SHA-256 |
|---|---|
| `carlito/Carlito-Regular.ttf` | computed on demand (see test) |
| `carlito/Carlito-Bold.ttf` | |
| `carlito/Carlito-Italic.ttf` | |
| `carlito/Carlito-BoldItalic.ttf` | |
| `caladea/Caladea-Regular.ttf` | |
| `caladea/Caladea-Bold.ttf` | |
| `caladea/Caladea-Italic.ttf` | |
| `caladea/Caladea-BoldItalic.ttf` | |

## Planned additions (see `STAGE-4-RENDER-FIDELITY.md`)

- Arial → Liberation Sans / Arimo (Apache-2.0 or OFL)
- Times New Roman → Liberation Serif / Tinos
- Courier New → Liberation Mono / Cousine
- Optional CJK → Noto Sans CJK (OFL) or metric tables/fallback

## License of the project

The font files keep their own licenses (above). The rest of the repository is
MIT OR Apache-2.0.
