# Generation postprocessing and centered dialogs

2026-10-20, branch codex/image-studio, base 7eab344c. Changes are uncommitted.
Only image-editor changed; StrictLib/model-runtime sources are unchanged.
The graph points at the original D:/ checkout and has no coverage for these files;
verification uses direct worktree source, tests and native execution.

## Contract

UI enables generation → Img2Img → UltraSharp-1x by default. Denoise 0.2, refiner
scheduler steps 20, UltraSharp source tile 256, GPU selected. Denoise/steps/device
and local weights are editable, and the complete postprocessing chain can be disabled.
A random seed is resolved once before starting; refinement inherits the actual
primary seed, model, prompt/negative, LoRA and VAE. Refiner output size is the
primary result size. Final Candidate retains original request mode/output geometry
and original source snapshot, so the existing stale-source acceptance check applies.

Postprocess settings serialize into Request.postprocess only on a completed
pipeline candidate; PSD recipe roundtrip retains them. Legacy Request JSON without
this field remains single-pass. NativeJob/Job are one-pass low-level backends and
refuse requests containing postprocess; callers use PipelineJob instead.

Pipeline worker runs outside UI, starting each backend through the same host Runtime.
It holds no outer inference lease across nested jobs. Each backend keeps cleanup
ownership on cancellation; model admission/retry/eviction retain the R03–R05 contract.
Native BUSY permit now drops before publishing its result, removing a race when the
next Img2Img stage starts immediately. A single final preview is published; errors
or cancellation do not apply intermediate images or mutate document/Undo. Missing
UltraSharp weights fail before primary compute. Inpaint restores protected pixels
again after the complete chain, including inversion/soft-mask compositing.

UltraSharp --keep-size uses actual 4x inference per padded tile. Each predicted
padded tile is clamped/converted to RGB8 and Lanczos-downsampled to source resolution
before cropping/pasting its core. Context around tiles is retained during filtering.
No full-frame 4x tensor is allocated in this mode; normal 4x behavior stays unchanged.
Alpha is copied exactly from input in 1x mode. This is not a separate 1x neural model,
and tiling uses the existing finite overlap. Keep-size is refused for AuraSR/blend.

All nine egui Window dialogs use CENTER_CENTER anchors: model/LoRA/VAE/Civitai browser,
generation preview, new canvas, layer name, memory, BiRefNet, SAM, upscale and unsaved
project confirmation. Native rfd file/folder pickers have the demo Frame as parent.
Those system picker windows were not driven visually in this acceptance; native
placement remains OS-managed. An embedding host owns FileAction and must set its own
native picker parent. Widget dropdowns/context popups remain attached to their controls.

## Verification

- postprocess-tests-final.log: **58 PASS**, 0 failures, all targets with
  demo,plakat-cuda,ollama (31 unit + 11 editing + 7 manual + 6 PSD + 3 runtime).
- postprocess-clippy-cuda.log: all targets, same features, -D warnings PASS.
- Python AST and keep-size argument validation PASS, including AuraSR refusal.
- Unit tests: exact primary/refiner/sharpener order; refinement uses generated pixels;
  original source/seed/geometry retained; post-disabled and legacy requests stay single
  pass; cancellation after each stage publishes no candidate; sharpening failure leaves
  document unchanged; inpaint protected RGBA restored; PSD stores the complete recipe;
  missing model never enters runtime queue.
- postprocess-smoke.log: real RTX 5070 Ti, SDXL prepared local/image-studio-sdxl,
  512x512, primary 20 steps, seed42, refiner denoise0.2/20 scheduler steps, UltraSharp
  safetensors/tile256/CUDA. Full chain completed in **18.356 s** after warm baseline.
  This is an execution measurement for this case, not a general performance guarantee.
  Final size 512x512, final pixels differ from baseline, PSD recipe roundtrip exact.
- Real same-size UltraSharp RGBA 257x129, tile128 (multiple tiles): dimensions exact,
  every alpha byte retained at123; worker reused weights (loads1/hits1), GPU-reserved
  bytes reported 2,023,751,680. Explicit release leaves runtime resident registry empty.
- postprocess-before.png / postprocess-after.png inspected: one blue vase remains
  on the table, details/edges change, geometry/frame size retained. The earlier 4-step
  smoke was only an execution check; the final receipt uses the ordinary 20-step input.
- Native CUDA demo built and opened postprocess-result.psd. centered-memory-1440.png
  and centered-model-1100.png inspected: dialogs centered at viewport (720,450) and
  (550,350), respectively. Files/weights list scrolls within the model dialog.

## Reproduce

```powershell
. ./image-editor/scripts/enter-cuda.ps1
cargo +1.92.0 clippy -p image-editor --all-targets --features demo,plakat-cuda,ollama `
  --config 'profile.dev.package.plakat.opt-level=2' --config 'profile.dev.package.sha2.opt-level=3' -- -D warnings
cargo +1.92.0 test -p image-editor --all-targets --features demo,plakat-cuda,ollama `
  --config 'profile.dev.package.plakat.opt-level=2' --config 'profile.dev.package.sha2.opt-level=3'
cargo +1.92.0 run -p image-editor --example postprocess_smoke --features demo,plakat-cuda,ollama `
  --config 'profile.dev.package.plakat.opt-level=2' --config 'profile.dev.package.sha2.opt-level=3'
```

Artifacts live under image-editor/artifacts (ignored): before/after PNG, PSD with full
recipe, logs, centered-dialog screenshots and postprocess-receipt.json with SHA256.
Weights are local, never included in project files or Git. No commit created.
