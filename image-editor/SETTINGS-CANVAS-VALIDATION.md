# Settings and canvas acceptance — 2026-10-20

Scope: image-editor only, worktree `codex/image-studio`, base `dca0ffa2`.
No StrictLib library or shared model-runtime implementation changes.

## Implemented

- Common centered settings window and reusable host settings page: generation,
  backend/postprocessing, BiRefNet, SAM, UltraSharp/AuraSR paths/download/device/tile.
- Editing panel: action controls only. Upscale panel: blend weights and action.
- Versioned preference snapshot, atomic demo persistence and startup restore of model
  identifier/name/family/cache, dimensions, adapters, VAE, tool paths and parameters.
  Prompt/negative, Civitai credentials and project pixels are excluded.
- Arbitrary clockwise image rotation; expand or keep/crop canvas bounds.
- Centered resize dialog: linked aspect dimensions, scale, fit, fill/crop,
  bounds-only padding/crop, 3×3 anchor. Applies to every layer and shared mask.
- Geometry validates pixel/layer memory budget before output allocation, preserves
  exact orthogonal/bounds pixels including invisible RGB, and interpolates rotation
  colors with premultiplied alpha. UI uses a single undo checkpoint on Apply.

## Validation

`cargo +1.92.0 clippy -p image-editor --all-targets --features demo,plakat-cuda,ollama --config 'profile.dev.package.plakat.opt-level=2' --config 'profile.dev.package.sha2.opt-level=3' -- -D warnings`

`cargo +1.92.0 test -p image-editor --all-targets --features demo,plakat-cuda,ollama --config 'profile.dev.package.plakat.opt-level=2' --config 'profile.dev.package.sha2.opt-level=3'`

`cargo +1.92.0 build -p image-editor --bin image-editor-demo --features demo,plakat-cuda,ollama --config 'profile.dev.package.plakat.opt-level=2' --config 'profile.dev.package.sha2.opt-level=3'`

Run after sourcing `scripts/enter-cuda.ps1`: strict Clippy PASS; 68 tests PASS
(33 library, 2 demo persistence, 11 editing, 7 manual, 6 PSD, 3 runtime, 6 transforms);
CUDA demo build PASS. Existing pipeline/PSD/runtime regression tests included.
`git diff --check` PASS.

New tests cover actual preference write/overwrite/read, malformed/oversized input,
selection restore across new editor/project, missing weights retained, invalid
configuration rejected atomically; clockwise mask orientation, lossless 90-degree
roundtrip, anchored crop/padding, fit vs fill, arbitrary-angle expand/clip,
transparent edge colors, undo/redo, invisible RGB and allocation-limit rejection.

## Native UI review

Captured the actual built demo at 1440×900 and 1100×700 using isolated
`IMAGE_STUDIO_SETTINGS` files under `image-editor/artifacts/`; no user preference profile changed.
Reviewed generated PNGs, including compact settings and geometry dialogs.

- `artifacts/clean-edit.png`: action-only editing controls.
- `artifacts/clean-upscale.png`: blend sliders and launch, no paths/device/download.
- `artifacts/clean-settings.png`, `clean-settings-1100.png`: centered settings,
  fixed tabs and scrollable content fitting the minimum window.
- `artifacts/clean-canvas.png`, `clean-canvas-1100.png`: centered dimensions,
  mode/anchor controls and Apply/Cancel.
- `artifacts/clean-rotate.png`, `clean-rotate-1100.png`: centered angle/expand dialog.
- `artifacts/clean-model-restored.png`: fresh native process restores saved
  `local/restore-checkpoint`, SDXL label and 1024×768 output despite environment
  default `IMAGE_STUDIO_MODEL=sd15`. The synthetic identifier exercises selection
  restoration; it is not an inference/model-weight loading claim.

Model weights are loaded on demand through the existing shared runtime.
GPU inference and new download requests were not run for this UI/geometry change;
there is no claim of new hardware-model acceptance. Previous postprocessing
acceptance is recorded separately in POSTPROCESS-VALIDATION.md.

Graph project `D-projects-StrictLib-mufoed`, generation `2026-10-19T19:03:34Z`,
has missing freshness for image-editor paths (this code lives in the worktree).
Graph discovery and coverage were checked; implementation conclusions use direct
source reads, tests and native captures, not graph completeness claims.

## Ctrl + wheel follow-up

Canvas zoom now uses egui's Ctrl-wheel zoom delta only while the pointer is over
its viewport, with scroll compensation to retain the source point under the
cursor (subject to centered-image and scroll-bound limits). Range: 2–800%.
Plain wheel remains scroll; view changes do not edit pixels or create history.
Three input-event tests PASS: pointer anchoring/document unchanged, no Ctrl or
outside viewport, wheel down and both bounds. All-target strict Clippy PASS;
CUDA demo rebuilt. Graph coverage for canvas_navigation.rs is missing, so this
change was verified from source and the input-event tests.
