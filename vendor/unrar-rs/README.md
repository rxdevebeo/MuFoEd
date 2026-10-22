# unrar-rs

A pure Rust port of the [UnRAR](https://www.rarlab.com/rar_add.htm) extraction
library. It reads and extracts RAR archives without linking to the original
C++ code or requiring any native dependency.

## Features

- RAR 5.0 / 7.0 archives
  - store, fastest, fast, normal, good and best compression methods
  - solid archives
  - LZ, delta, E8/E8E9 and ARM filters
  - CRC32 and BLAKE2sp checksums
  - AES-256 file data encryption and encrypted headers (PBKDF2-HMAC-SHA256)
  - multi-volume archives with automatic next-volume discovery
  - Unix symlinks, junctions, hard link/file copy metadata
  - file versions, timestamps, Unix owners, service records
- RAR 1.5 - 4.x archives
  - stored files
  - RAR 3.x LZ decompression (method 29) with standard filters
  - solid and multi-volume archives
  - AES-128 encrypted file data (RAR 3.0+)
- RAR 1.4 signature detection

## Usage

```rust
use unrar_rs::Archive;

fn main() -> unrar_rs::Result<()> {
    let mut archive = Archive::open("example.rar")?;

    for entry in &archive.entries {
        println!("{} ({} bytes)", entry.name, entry.unpacked_size);
    }

    archive.extract_all("output")?;
    Ok(())
}
```

Read a single entry into memory:

```rust
let mut archive = unrar_rs::Archive::open("example.rar")?;
let data = archive.read(0)?;
```

Encrypted archives:

```rust
let mut archive = unrar_rs::Archive::open_with_password("secret.rar", "hunter2")?;
archive.test()?;
```

Archives whose headers are not encrypted can be opened without a password and
the password can be supplied later:

```rust
let mut archive = unrar_rs::Archive::open("encrypted-files.rar")?;
archive.set_password("hunter2");
archive.extract_all("output")?;
```

Opening options, including the SFX signature search limit:

```rust
use unrar_rs::{Archive, ArchiveOptions};

let options = ArchiveOptions::default()
    .with_password("hunter2")
    .with_max_sfx_scan(8 * 1024 * 1024);
let mut archive = Archive::open_with_options("archive.rar", options)?;
```

The default `max_sfx_scan` is 4 MiB, matching UnRAR's `MAXSFXSIZE`. It only
affects how far from the beginning of the file the embedded RAR signature is
searched; the direct signature at offset 0 is always detected.

The `unrar` example provides a small command line interface:

```text
cargo run --example unrar -- list    archive.rar
cargo run --example unrar -- test    archive.rar [-p password]
cargo run --example unrar -- extract archive.rar output [-p password]
cargo run --example unrar -- cat     archive.rar entry-name [-p password]
```

## Limitations

- RAR 3.x PPM-compressed blocks are not implemented. Such entries return
  `Error::Unsupported`. The standard RAR 3.x filters (E8/E8E9, Itanium, delta,
  RGB and audio) are supported.
- RAR 2.x and RAR 1.5 decompression is not implemented (stored files still
  work).
- Encrypted RAR 4.x headers and RAR 1.x/2.x encryption are not supported.
- Each entry's packed data is buffered in memory before decompression (and
  decrypted in place for encrypted entries). Very large files require memory
  proportional to their packed size.
- On Windows, symlink and hard-link entries are created as empty placeholder
  files. On Unix, symlinks are created directly.
- Archive comments are parsed as service records but not exposed as text yet.
- The decompressor requires a contiguous dictionary allocation. Archives with
  dictionaries larger than available memory return an error instead of using
  UnRAR's fragmented-window fallback.
- Solid archives are decoded sequentially. Random access to a later solid
  entry decodes the preceding entries again.

## Safety

The crate contains **no `unsafe` code**; `lib.rs` declares
`#![forbid(unsafe_code)]`. The decompressors operate on safe slices and `Vec`s,
and sizes read from headers are validated or allocated fallibly before use.

## Tests

The crate has unit tests for the hash/KDF primitives and integration tests for
RAR 5 (including solid, filters, encryption and multi-volume), RAR 3.x LZ,
solid archives, standard filters, encryption, Unicode names and missing-volume
handling:

```text
cargo test
```

See `test-data/README.md` for the provenance of the test archives.

## License

This is an independent Rust port of the UnRAR source. UnRAR source code may be
used in any software to handle RAR archives without limitations free of charge,
but cannot be used to develop a RAR (WinRAR) compatible archiver and to
re-create the RAR compression algorithm, which is proprietary. Distribution of
modified UnRAR source code in separate form or as a part of other software is
permitted, provided that the full license text is included in the documentation
or source comments of the resulting package. See [LICENSE](LICENSE).
