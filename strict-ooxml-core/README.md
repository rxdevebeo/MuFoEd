# strict-ooxml-core

Resource-safe raw layer for reading **WordprocessingML Strict** packages
(ISO/IEC 29500-1:2008). This crate implements the OPC/ZIP packaging layer, a
streaming namespace-aware XML reader with hard resource limits, the
Strict ↔ Transitional namespace registry, conformance detection and the unified
error model.

It deliberately does **not** build a WordprocessingML DOM and does **not** render
anything; those concerns belong to later stages.

## Status

Stage 1 (`STAGE-1-TASK.md`): OPC/ZIP, content types, relationships, XML,
namespaces, conformance detection, limits, errors, CLI skeleton, tests and fuzz
targets.

## Example

```rust
use strict_ooxml_core::opc::{OpenOptions, Package};

let package = Package::open_path("document.docx", &OpenOptions::default())?;
println!("conformance: {:?}", package.conformance());
for part in package.parts() {
    println!("{}", part.id);
}
# Ok::<(), strict_ooxml_core::error::StrictError>(())
```

## Safety properties

- No `unsafe` (workspace lint `unsafe_code = "deny"`).
- No panics on hostile input: every failure is a `Result`.
- DTD and external entities are rejected; ZIP and XML are fuzzed.
- Default resource limits are always on and overridable via `OpenOptions`;
  they bound the compressed input size, ZIP entries, total/single uncompressed
  sizes, compression ratio, XML depth/attributes/text and part count.
- An optional `RawNormalizer` (Stage-6 seam) is applied to every part read
  before namespace resolution.

## License

MIT OR Apache-2.0.
