# PSD project format: first supported profile

Historical receipt: E03.2 extends this profile with standard raster layer masks
and Normal/Multiply/Screen/Overlay. Current acceptance and remaining exclusions:
[E03-LAYERS-VALIDATION.md](E03-LAYERS-VALIDATION.md).

2026-10-20, worktree `codex/image-studio`, baseline `cc7ae2f3`.
Only image-editor and root dependency lock changed; StrictLib sources unchanged.
Graph generation 2026-10-19T19:03:34Z points at the original D:/ checkout and
does not cover these files; verification used direct worktree source and tests.

## Implemented behavior

- Demo Save defaults to image-project.psd. Open PSD and command-line/BAT startup
  accept PSD; legacy JSON remains readable and explicitly writable by API.
- `Document::to_psd/from_psd`; `open_project` dispatches by magic, `save_project`
  by extension. Unsupported save extensions fail instead of producing mislabeled files.
- PSD v1 RGB8 raster layers, alpha, Unicode names, bottom-to-top order, visibility,
  byte opacity, full pixel/position lock, cropped bounds within canvas, merged preview.
- PSD resources include a named Adobe plug-in block, ID 4000 / MuFoEd.Project:
  zlib JSON for active layer, common inpaint mask and generation recipes. Pixel data
  stays in standard PSD channels. The block carries a SHA256 of layer properties and
  pixels, so externally edited layers cannot receive stale mask/recipe metadata.
  External tools may remove this block; then a standard raster project is imported
  without the MuFoEd mask/recipes. Existing-but-stale/duplicate/corrupt metadata fails.
- ag-psd 0.3.0, MIT, pinned exactly. Writer uses ZIP channel compression and an alpha
  layer even at the bottom. Input header/dimensions/layer count/resources are checked
  before decoding; bitmap allocation budget 128 MiB. Codec panics become errors.
- File replacement uses a same-directory temporary file, sync_all and atomic persist.
  Failed save leaves the prior destination unchanged. Failed open never replaces UI document.

## Deliberate unsupported features

Groups, layer masks, text/effects, smart objects, adjustment/vector layers,
non-normal blends/non-neutral Blend If ranges, partial locks, off-canvas pixels,
embedded ICC profiles, CMYK/16/32-bit and PSB fail with an explicit error.
There is no flatten-on-import fallback and no claim of full Photoshop roundtrip.
Inpaint mask remains a MuFoEd regeneration mask, not Photoshop layer transparency.
ICC rejection is conservative: conversion/colour-profile preservation is not implemented.
Current document limits remain 16 layers / 16,777,216 pixels / 128 MiB resident pixel
buffers. Input/output file limit 128 MiB. Codec working buffers add to document memory.

## Evidence

- `artifacts/psd-tests-final.log`: 53 PASS, zero failures (26 unit + 11 editing +
  7 manual + 6 PSD + 3 runtime integration), all demo targets.
- `artifacts/psd-clippy.log`: all targets/demo, -D warnings PASS.
- `artifacts/psd-clippy-cuda.log`: all targets/demo,plakat-cuda,ollama, -D warnings.
- `artifacts/psd-smoke.log`: real four-layer 960x640 workbench exported as PSD,
  read back to exact Document including mask/recipe/state. `psd-workbench.psd` and
  per-layer PNG/merged PNG/receipt remain in artifacts (not committed).
- `artifacts/psd-independent.log`: psd-tools 1.25.0 independently opened our PSD;
  exact RGBA for all four layers, names, order, visibility, opacity and stored merged
  image matched the exported PNGs. Independent reader accepts the private resource.
- Checked-in tiny external fixtures created by psd-tools/Pillow: RGB8 with RLE/ZIP,
  2x1 foreground offset at (1,1), opacity173; Multiply, actual layer mask and RGB16.
  Positive import and explicit negative cases run without Python in Rust tests.
- Rust roundtrip also covers nonzero inpaint mask, u64::MAX seed, generation recipe,
  hidden/zero-opacity layer with nonzero RGB under alpha0, Cyrillic/CJK/emoji names,
  active/locked layer and exact composited pixels. Metadata mismatch/duplicate,
  every truncated prefix, invalid dimensions/depth/version and atomic failure tested.
- `artifacts/psd-demo-build.log`, `psd-ui.log`, `psd-ui-1440.png`: native CUDA demo
  opens the saved PSD as four layers. Screenshot inspected for content and PSD controls.
  Desktop Photoshop/Paint.NET roundtrip was not run; interoperability evidence is
  independent psd-tools parsing/writing, not acceptance in those desktop applications.

## Reproduce

```powershell
cargo +1.92.0 test -p image-editor --all-targets --features demo
cargo +1.92.0 clippy -p image-editor --all-targets --features demo -- -D warnings
cargo +1.92.0 run -p image-editor --example psd_smoke -- `
  image-editor/artifacts/manual-workbench.json image-editor/artifacts/psd-workbench.psd
# Optional independent QA, isolated packages only:
py -3.11 -m pip install --target image-editor/.runtime/psd-qa --no-deps psd-tools==1.25.0 aggdraw
$env:PYTHONPATH = (Resolve-Path image-editor/.runtime/psd-qa).Path
py -3.11 image-editor/scripts/validate_psd.py image-editor/artifacts/psd-workbench.psd
# Regenerate independent fixtures deliberately:
py -3.11 image-editor/scripts/validate_psd.py --fixtures image-editor/tests/fixtures/psd
```

CUDA demo verification uses the same toolchain/config as R03–R05:

```powershell
. ./image-editor/scripts/enter-cuda.ps1
cargo +1.92.0 build -p image-editor --bin image-editor-demo --features demo,plakat-cuda,ollama `
  --config 'profile.dev.package.plakat.opt-level=2' --config 'profile.dev.package.sha2.opt-level=3'
$env:IMAGE_STUDIO_SHOT = "$PWD/image-editor/artifacts/psd-ui-1440.png"
& ./target/debug/image-editor-demo.exe image-editor/artifacts/psd-workbench.psd
```

References: [Adobe PSD specification](https://www.adobe.com/devnet-apps/photoshop/fileformatashtml/),
[ag-psd 0.3.0](https://docs.rs/ag-psd/0.3.0/ag_psd/),
[psd-tools](https://psd-tools.readthedocs.io/).
