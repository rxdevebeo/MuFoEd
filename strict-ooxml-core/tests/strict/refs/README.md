# Reference renders (WPS Office)

Per-document reference PNGs used by the SSIM gate
(`strict-ooxml-render-svg/tests/ssim.rs`,
`STAGE-4-RENDER-FIDELITY.md` S4F.4/S4F.5).

## Layout

```
refs/<document>/page_N.png   # N = 1-based page
```

`<document>.docx` is the matching fixture in `tests/strict/`.

## Generator (pinned)

| Item | Value |
|---|---|
| Engine | WPS Office `12.1.0.28485` (`kwpsconvert.exe`) |
| Command | `kwpsconvert.exe word2photo --input <docx> --output <dir>` |
| Output | `<dir>/<stem>/page_N.png` |
| DPI / page size | 96 DPI; page size from `w:sectPr` (Letter here: 816×1056) |

Background: this WPS build was verified to **read Strict** documents and emit
one PNG per page. References are generated **once** on this pinned version and
committed; WPS is **not** required in CI. Regenerating on a different version is
not reproducible and is out of scope.

## Documents

| Document | Compared how | Notes |
|---|---|---|
| `strict-text` | per-page **SSIM ≥ 0.95** + page count | Text-only synthetic Strict doc exercising Carlito/Arimo/Tinos/Cousine text, justification and centering. |
| `strict-profile` | **page count only** (2 pages) | Real Word-produced Strict file (`kklimuk/docx-cli`, MIT). Dominated by a DrawingML chart and diagram, which Stage 4 cannot rasterize, so per-pixel SSIM is not meaningful. |

Rasterization of our SVG uses `resvg` with the bundled fonts in
`strict-ooxml-render-svg/assets/fonts/` (see its `ATTRIBUTION.md`).
