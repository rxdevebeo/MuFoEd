# egui-winit 0.35.0

Upstream: <https://github.com/emilk/egui> (`crates/egui-winit`), published on
crates.io as `egui-winit` 0.35.0. Licence: MIT OR Apache-2.0, as its
`Cargo.toml` declares (the published package carries no licence files; the
texts are in the upstream repository).

One patch, marked `MuFoEd patch:` in `src/lib.rs`: Ctrl+V with no text on
the clipboard is reported as a key press instead of being dropped, so the
editor can paste a picture.
