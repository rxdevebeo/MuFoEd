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
