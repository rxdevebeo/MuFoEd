# RC02 reviewed font / script support profile

Date: 2026-10-08. Card: RC02 (`docs/rtf/compatibility/RC02/STATUS.json`).
Contract and limits: [FONT_RESOURCE_CONTRACT.md](../FONT_RESOURCE_CONTRACT.md).

This is the **reviewed** script/font support profile the RC02 contract asks for.
It states, per script and delivery path, what is supported, what is degraded, and
what is explicitly unsupported — and on which independent evidence. It does not
upgrade any finite witness into general script support.

## Evidence basis

- Default bundled deterministic faces (Arimo/Tinos/Carlito/…), family aliases and
  substitution: full SVG/PDF/public suites, plus the pinned alias regression
  [font-alias.verification.json](../font-alias.verification.json) (ArialMT and
  TimesNewRomanPSMT extracted through an independent `pypdf` reader; no system
  font payload is copied).
- Explicit assets: `FontAssetRegistry` admission (hash/face/index/variation/OS/2),
  `FontResource` metrics/shaping, and the SVG/PDF delivery witnesses.
- CJK witness: `fonts/witnesses/cjk.otf` (renamed finite OFL derivative of Noto
  Sans CJK SC), geometric oracle = native HarfBuzz 14.6.0, plus strict
  SVG/PDF/RTF delivery and PDF Unicode extraction.
- Devanagari witness: `fonts/witnesses/devanagari.ttf` (renamed finite OFL
  derivative of Noto Sans Devanagari), same independent geometry oracle, plus the
  ActualText extraction witness (MuPDF 1.26.7 / PyMuPDF 1.26.4).
- Admission/denial behaviour: `RenderOptions::font_failure_policy`
  (`ReportAndContinue` default, `Reject` strict) and the bounded reports.

## Profile

| Script / family class | WML slot(s) | Delivery status | Independent evidence | Explicit limits |
| --- | --- | --- | --- | --- |
| Latin, Cyrillic, Greek | `w:ascii`/`w:hAnsi` (RTF `\loch`/`\hich`/`\f`) | **Supported** | Bundled faces; alias regression; PDF text extraction | Deterministic bundled faces, not the machine's fonts |
| East Asian (CJK) | `w:eastAsia` / `w:eastAsiaTheme` (RTF `\dbch` then `\af`) | **Supported (finite witness coverage)** | `cjk.otf`, native HarfBuzz geometry, strict SVG/PDF/RTF, ActualText extraction | Only the pinned subset text; mixed Latin/CJK keeps the base face; default bundled substitution is *not* CJK coverage |
| Complex script (Devanagari) | `w:cs` (RTF `\rtlch`/`\ltrch`, `\fcs0/1`) | **Degraded** | `devanagari.ttf` geometry + embedding pass; ActualText extraction passes | harfrust reports `font.script.degraded`; strict rendering refuses; continuation CIDs need ActualText-aware readers |
| Other complex scripts (Arabic, Hebrew/RTL, Tamil, Bengali, Thai, …) | `w:cs`, `w:bidi` | **Unsupported** | No positive witness exists | `\alang`/bidi language mapping lands, but shaping/visual acceptance is RC03 |
| Symbol / symbol-encoded faces | `w:sym`/`w:cs` | **Unsupported** | No independent witness | Do not synthesize coverage from a bundled fallback |
| Variable / collection faces | any slot | **Supported for delivery** | `FontResource` key = program SHA-256 + face index + variation coordinates; registry admission | SVG rejects collections (including TTC index 0); PDF subsets the selected face and every coordinate; visual acceptance pending |
| Missing / unresolved request | any slot | **Degraded** (default) / **Rejected** (strict) | `FontFailurePolicy`; bounded reports expose requested/resolved family | Never silently substituted as exact; no system discovery |

## Reader compatibility decision

Multi-glyph clusters carry logical UTF-16 `/ActualText` around the painted CIDs.
Spec-compliant readers that honor ActualText (MuPDF/Adobe/PDFium) extract the
logical text exactly; this is the RC02 extraction witness. Readers that ignore
ActualText (for example the `lopdf` text helper used as an independent oracle) fall
back to per-CID `ToUnicode` and show replacement characters for continuation CIDs.
That is a documented reader limitation, not an unresolved renderer defect: the
renderer emits a spec mechanism, and universal reader compatibility is not claimed.
Our own `strict-ooxml-pdf` layer is a coarse glyph layer and is not used as the
exact-extraction oracle.

## Non-claims

- A finite pinned witness does **not** establish general CJK or Indic support.
- An exact family binding does **not** prove internal naming, style authenticity,
  glyph coverage or full script support.
- Equal page counts or a close SSIM do **not** constitute visual acceptance.
- Bundled default resources continue to be exercised by the corpus CLI; explicit
  assets are opt-in via `RenderOptions::font_assets` / `--fonts`.

## Review decision

Font resolution, registry admission, owned shaping, and exact SVG/PDF delivery are
complete for the finite witnesses above. The remaining RC02 acceptance items —
full complex-script placement/visual acceptance and full-corpus visual acceptance —
are gated on shaping and text metrics (RC03/RC04) and on the external pixel harness;
they are recorded as Blocked in `STATUS.json`, not as font-resolution defects.
