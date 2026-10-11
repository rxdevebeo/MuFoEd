# EMF SDK for Rust

[![crates.io](https://img.shields.io/crates/v/emfsdk.svg)](https://crates.io/crates/emfsdk)
[![docs.rs](https://docs.rs/emfsdk/badge.svg)](https://docs.rs/emfsdk)

`emfsdk` is a pure-Rust library for reading, editing, and writing EMF, EMF+,
and WMF metafiles. Known records are typed; unknown records, extensions,
padding, strings, and opaque payloads retain the bytes needed for round trips.

## Usage

```bash
cargo add emfsdk
```

```rust,no_run
use std::{
    fs::{self, File},
    io::{BufWriter, Write},
};

use emfsdk::Metafile;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let input = fs::read("input.emf")?;
    let metafile = Metafile::from_bytes(&input)?;

    println!("{:?}", metafile.format());

    let mut output = BufWriter::new(File::create("output.emf")?);
    metafile.write_to(&mut output)?;
    output.flush()?;
    Ok(())
}
```

Use `EmfMetafile`, `WmfMetafile`, and record `parse_data` methods for typed
format-specific access. `rebuild_typed` reconstructs a record through its typed
writer while preserving its wire metadata.

For read-only work, `MetafileRef`, `EmfMetafileRef`, `WmfMetafileRef`, and
`EmfPlusStreamRef` validate framing and borrow record payloads from the input.
Their iterators allocate nothing. Call `into_owned` before editing or writing.

Top-level `write_to` methods accept any `std::io::Write`; output does not need
to implement `Seek`. Use `to_bytes` only when an in-memory file image is
required.

## Compatibility

`Metafile::from_bytes` is compatibility-first. Producer-specific reserved
values and bounded extensions are preserved and reported by
`compatibility_diagnostics`.

Use `Metafile::from_bytes_strict` or `validate_strict` when every known field
must conform to the Microsoft specifications. Unknown record types remain an
explicit raw fallback; malformed known records are not silently accepted as
typed data.

## Features

- Default: parsing and writing only
- `render`: raster rendering with `image`, `fontdb`, and `ttf-parser`

```bash
cargo add emfsdk --features render
```

The minimum supported Rust version is 1.88. The crate uses Rust 2024.

## Specifications and testing

- [[MS-EMF]](https://learn.microsoft.com/en-us/openspecs/windows_protocols/ms-emf/e0137630-f3ad-492c-bde9-e68866e255ba)
- [[MS-EMFPLUS]](https://learn.microsoft.com/en-us/openspecs/windows_protocols/ms-emfplus/229f98d8-c19a-464e-80cc-2cb96aba1d71)
- [[MS-WMF]](https://learn.microsoft.com/en-us/openspecs/windows_protocols/ms-wmf/4813e7fd-52d0-4f42-965f-228c8b7488d2)

Corpus tests in
[`ooxmlsdk-test-suite`](https://github.com/KaiserY/ooxmlsdk-test-suite)
exercise typed parse/write and exact whole-file comparison against standalone
and Office-embedded metafiles. Compatibility fallbacks and failures are
counted, not skipped.

## Status

The crate is pre-1.0 and its API may still change. The optional renderer targets
portable previews, not pixel-identical Windows GDI/GDI+ output.

See [CHANGELOG.md](./CHANGELOG.md) for release notes.

## License

MIT OR Apache-2.0
