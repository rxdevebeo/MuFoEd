# strict-ooxml

Rust toolkit for reading, normalizing and rendering **WordprocessingML Strict**
documents (ISO/IEC 29500-1:2008), with controlled normalization of Transitional
(ISO/IEC 29500-4:2008) input.

Design documents live in the repository root:

- `TZ-STRICT-OOXML-RUST.md` — full technical specification.
- `base_target.md` — the original (1.0) specification.
- `STAGE-1-TASK.md`, `STAGE-1-REWORK.md` — Stage-1 task and rework.
- `STAGE-2-TASK.md`, `STAGE-2-REWORK.md` — Stage-2 task and rework.
- `docs/stage-2-report.md` — Stage-2 delivery report.
- `docs/adr/` — accepted architecture decision records (incl. ADR-0004).

## Workspace layout

| Crate | Purpose |
|---|---|
| `strict-ooxml-core` | OPC/ZIP, XML, namespaces, limits, errors (Stage 1). |
| `strict-ooxml-wml` | WordprocessingML Strict DOM and parser (Stage 2). |
| `strict-ooxml` | Public `StrictDocument` API (Stage 2). |
| `strict-ooxml-cli` | `inspect` / `check` command-line tool. |
| `xtool` | Dev utility: XSD inventory, coverage gate, `.docx` generator. |

Later stages add `strict-ooxml-report` and `strict-ooxml-render-svg`
(`TZ` §5.1).

## Build and test

```text
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo build --workspace --all-targets --all-features
cargo test --workspace --all-features
cargo doc --workspace --no-deps
```

Coverage (≥ 80% lines for `core` and `wml`):

```text
cargo llvm-cov -p strict-ooxml-core --all-features --fail-under-lines 80
cargo llvm-cov -p strict-ooxml-wml --all-features --fail-under-lines 80
```

Optional-element coverage gate (≥ 90%) and the independent corpus cross-check:

```text
cargo run -p xtool -- coverage --file coverage/wml-elements.toml --min 90
cargo run -p xtool -- corpus-elements
```

Fuzzing (requires `cargo-fuzz` and a nightly toolchain):

```text
cargo +nightly fuzz run fuzz_zip
cargo +nightly fuzz run fuzz_xml
cargo +nightly fuzz run fuzz_relpath
cargo +nightly fuzz run fuzz_wml
```

## License

MIT OR Apache-2.0.
