# Component provenance

| Component | Source | Derivation |
| --- | --- | --- |
| IFF, INFO, DIRM, INCL | DjVu Reference v3; [transcription](https://www.sndjvu.org/spec.html) | Original bounded implementation in this crate |
| TXTa/TXTz zone tree | Same published specification; STAGE-9-TASK §5 delta rules | Original UTF-8/range/zone implementation |
| BZZ/ZP | [matyushkin/djvu-rs 0.40.2](https://github.com/matyushkin/djvu-rs), MIT | BZZ vendored from crates.io; retain upstream LICENSE. Added caller block/output budgets before allocation |
| JB2/bitmap | Same MIT upstream 0.40.2 | JB2 vendored; retain upstream LICENSE. Added canvas/record budget APIs and validation of start record; bitmap remains registry dependency |
| IW44/pixmap | Same MIT upstream 0.40.2 | Pinned registry subcrate; wrapper checks initial plane size before decode |
| PNG | Existing workspace `png` 0.18 | Lossless wrapping of RGB composite |
| DjVuLibre | External binaries only for independent test oracles | Not linked or shipped with production crates; no codec source consulted |

No DjVuLibre codec source was read in implementing this change. Byte-level
format corrections are recorded in STAGE-9-TASK §16. This provenance statement
applies to this implementation; upstream's clean-room statements remain their own.
Upstream license is MIT (copyright 2026 Lev Matyushkin); the exact copyright and
permission notice are preserved in each vendored LICENSE.

The IFF implementation is local rather than the planned `djvu-iff` adapter:
chunk/depth/work budgets and sharing of component source buffers are checked
as headers are consumed, before building a tree. No umbrella `djvu-rs`
dependency, GPL binding, tesseract, tract, or wasm runtime is introduced.

## Decoder extension

- FGbz byte layout and Smmr MMR header were cross-checked against MIT upstream
  `matyushkin/djvu-rs` `src/fgbz.rs` and `src/smmr.rs`; those sources, not
  DjVuLibre codec sources, were consulted. Our parsers are stricter and bounded.
- JPEG uses existing permissively licensed `zune-jpeg` 0.5.15 / `zune-core`;
  header dimensions are validated before decoding RGB pixels.
- G4 decoding uses existing MIT/Apache-2.0 `hayro-ccitt` 0.4.0. Its output
  adapter refuses truncated rows and bounds each callback to the declared canvas.
- Legacy IW44 follows DjVu Reference Appendix 1, using the same codec as BG44.
- Indexed JB2 gains caller canvas/record budgets and returns the full blit count,
  including off-canvas blits, to validate the palette index table.
- External oracle fixtures `navm_fgbz.djvu`, `legacy_pm44.djvu`,
  `legacy_bm44.djvu` are retrieved only into ignored scratch storage from the
  MIT upstream test fixtures; source URLs and SHA256 are in validation receipts.
