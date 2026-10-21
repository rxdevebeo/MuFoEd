# GPU LoRA merge and generation presets — 2026-10-20

Scope: image-editor only, preserving all existing worktree edits. No changes to
StrictLib, upstream Plakat or shared model-runtime implementation. No commit created.

## Diagnosis and implementation

The adapter previously called Plakat merge_loras_into_weights with Device::Cpu,
including in CUDA builds. Both reported LoRAs are valid SDXL files: add-detail-xl
(F32, rank 16, 228452344 bytes) and PerfectEyesXL (BF16, rank 32, 228453996 bytes).
CPU full-checkpoint merging plus no visible substage could appear to stall startup.
A timed old-path CPU baseline was not run, so no measured speedup ratio is claimed.

The merge now receives the actual admitted compute device, before constructing
UNet/CLIP GPU modules. GPU temporaries are released/synchronized before CPU cache
publication and subsequent model placement. Files.weights retains the shared
lease and exact model/LoRA fingerprint keys; the merge implementation signature
is versioned. Cached compatible merged CPU tensors continue to bypass recomputation.
The runtime implementation and admission policy were not modified.

Named merge stages and completion counts are exposed beside the generation spinner.
The pipeline remains cancellable between components; Plakat's current merge call
finishes before its buffers/lease can be released. Its temporary-file serialization
is retained; this is GPU arithmetic, not a disk-free new merge engine.

## Hardware evidence

Native CUDA test with pornzillaHentai_v45, full provided 85-token prompt,
1024×1024, DPM++ 2M Karras, 12 steps, CFG 7.5, seed 42, explicit SDXL VAE,
add-detail-xl weight **0.6**, PerfectEyesXL weight **0.7**, no postprocessing:

- UNet: **1444/1444** target applications, **13.5 s**, GPU.
- CLIP-L: **144/144**, **1.0 s**, GPU.
- CLIP-G: **384/384**, **3.2 s**, GPU.
- Complete native generation: **55.4 s**, success, runtime queue-release assertions pass.
- Output: `artifacts/lora-gpu-merge.png`.

Counts sum target applications across both LoRAs, not unique layer count.
Example native_smoke accepts IMAGE_STUDIO_SMOKE_LORAS as a JSON array to reproduce
multi-adapter runs. Current user model/LoRA files and settings were read only.

## Preset behavior

GenerationPreset is part of EditorSettings. Save, apply, named overwrite, delete,
JSON import/export and standalone host API implemented. Capture includes model,
adapters/weights/enabled state, VAE, sampler, output size, steps, guidance, seed and
random mode, strength, generation mode, mask and postprocessing parameters.
Prompt/negative are optional and excluded by default. Unchecked presets preserve
the currently entered text on application. Startup cache and document pixels remain
unchanged. Existing profile files without presets continue to load.

Preset limits are bounded and validated before mutation. Missing weights remain
referenced, active model operations block application, and colliding imported names
reject the whole import. Export and profile persistence use atomic replacement.
Named overwrite is explicit in the UI. JSON never embeds weights or credentials.

## Verification

Strict all-target Clippy PASS, 80 tests PASS (45 library, 2 demo, 11 editing,
7 manual, 6 PSD, 3 runtime, 6 transforms), CUDA demo build PASS. Features:
demo,plakat-cuda,ollama; cargo +1.92.0; dev package overrides Plakat opt-level=2,
sha2 opt-level=3; source scripts/enter-cuda.ps1 before building.

New behavioral tests check two-LoRA retention and settings restart roundtrip,
preset persistence across document replacement, seed u64, startup-cache/pixel
preservation, optional text, export/import and atomic duplicate/malformed rejection.
Native preset-window captures at 1440×900 and 1100×700 use an isolated QA profile:
`artifacts/presets-1440x900.png`, `artifacts/presets-1100x700.png`.

Graph project D-projects-StrictLib-mufoed generation 2026-10-19T19:03:34Z has
missing freshness for these worktree paths. Graph/coverage checked; implementation
and runtime contract inspected directly, then tests and native GPU execution used.
