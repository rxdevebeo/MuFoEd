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
| DPI | 96 DPI |
| Page sizes | `strict-text` 816×1056 (Letter); `strict-text-grid` 816×1056; `strict-profile` 727×1055 (page size from its `w:sectPr`, `545.30pt`×`791.90pt`) |

Background: this WPS build was verified to **read Strict** documents and emit
one PNG per page. References are generated **once** on this pinned version and
committed; WPS is **not** required in CI. Regenerating on a different version is
not reproducible and is out of scope.

## Documents

| Document | Compared how | Notes |
|---|---|---|
| `strict-text` | per-page **SSIM ≥ 0.95** + structural invariant + page count | Text-only synthetic Strict doc exercising Carlito/Arimo/Tinos/Cousine text, justification and centering. |
| `strict-text-grid` | per-page **SSIM ≥ 0.95** + structural invariant + page count | Same, plus `w:docGrid type="lines" linePitch="360"` and a Cambria/Caladea run; pins the grid line-height rule (`S4F-REWORK-2` B-1). |
| `strict-profile` | **page count only** (2 pages) | Real Word-produced Strict file (`kklimuk/docx-cli`, MIT). Dominated by a DrawingML chart and diagram, which Stage 4 cannot rasterize, so per-pixel SSIM is not meaningful (`S4F-REWORK-2` B-2). |

The structural invariant (`tests/ssim.rs`) additionally requires similar ink
coverage, a row-ink profile correlation ≥ 0.9 and no vertical drift beyond
2 px, which rejects a blank page or a ≥ 3 px shift that SSIM alone tolerates.

## SHA-256

| File | SHA-256 |
|---|---|
| `strict-text/page_1.png` | `4d081f5265e8e27bef852f5eebd332aa1ee5d37995acd007a02d6437ebc57e57` |
| `strict-text-grid/page_1.png` | `5978bf83866029cd04e38d52c718e0c492f1db036524036d9ece9795163e198c` |
| `strict-profile/page_1.png` | `652e8db9b083c33ea0a2d50eb205df74ffd9f04deed6ca6160d915d12d91fff3` |
| `strict-profile/page_2.png` | `c93812af2890ae6c50f490c8ac875e5acd4a90fdf646a90f0385e8a35abc8f7c` |

Rasterization of our SVG uses `resvg` with the bundled fonts in
`strict-ooxml-render-svg/assets/fonts/` (see its `ATTRIBUTION.md`).
