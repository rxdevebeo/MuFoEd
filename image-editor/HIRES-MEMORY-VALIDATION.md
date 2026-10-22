# Hires Fix VRAM admission — 2026-10-20

Scope: image-editor only; shared model-runtime/StrictLib/Plakat implementations unchanged.

Reported failure: requested 14,763,950,080 bytes (13.75 GiB), available
13,726,593,844 bytes (12.78 GiB). The old estimate evaluates exactly to this
request for SDXL at 1536 squared. This is admission refusal, not a CUDA allocation error.

Native high-resolution path (>1,048,576 pixels):
- Full-image negative/positive CFG branches run sequentially; hidden, pooled and
  SDXL time-id batch rows remain aligned. Sampler, prompts, seed, size and CFG remain selected.
- VAE encode/decode operate on 512px tiles / 128px overlap (64/16 latent units).
  Float accumulators and weight normalization live on CPU; only tiles and final
  tensors move through CUDA. Encode remains posterior sampling; overlap blends
  samples as well as local encoder outputs, so results are not identical to
  full-image posterior sampling. Norm/attention context is also tile-local.
- Cancel checked between tiles; non-finite/wrong-shape tiles fail without publication.
- SDXL native VRAM estimate: 6 GiB baseline + two F32 attention score buffers,
  10 heads and area/256 tokens, for one CFG branch. 1536 squared = 12.328125 GiB.
  Baseline includes UNet weights and other workspace, while VAE tiles bound its
  peak. SD1.5 uses its 1/8 grid and 8 heads; large sizes remain rejectable.
  CLI retains old estimate because it does not implement these changes.
- Shared runtime reserve, eviction, queue and CPU cache contracts remain intact.

CUDA acceptance, RTX 5070 Ti 16GB:
`HIRES_SMOKE_BASE=1024 cargo +1.92.0 run -p image-editor --example hires_smoke`
with standard demo,plakat-cuda,ollama features and local prepared SDXL.
1024x1024 / DPM++ 2M Karras / 10 steps / seed42 -> UltraSharp1536 -> Img2Img1536
/ 10 full-schedule steps / denoise0.3 / CFG6. Completed in 95.509s.
Final1536 PNG visually inspected (blue ceramic vase), no obvious tile grid,
PSD/request roundtrip equality, unchanged original document/source, final resident
release all PASS. Files: artifacts/hires-ultrasharp-img2img.png and .psd.
This does not establish exact Invoke parity or arbitrary large-image/LoRA support.

Tests cover rectangular/edge tile overlap ramps, encode/decode coordinate scaling,
output channels/dimensions, invalid/nonfinite tiles, initial and mid-tiling cancellation,
and architecture-specific quadratic admission estimates. Full tests, strict Clippy,
and demo CUDA build run with the established flags.

Telemetry limitation / TODO for the shared manager (separate scope): nvidia-smi
under WDDM returned transient used/free values larger than total during this run
(artifacts/hires-memory-samples.csv). Therefore these samples cannot establish a
physical VRAM peak. Validate/reject impossible measurements centrally before
claiming a strict physical-memory bound; no runtime-policy bypass was added here.

Graph coverage for this checkout is missing in D-projects-StrictLib-mufoed,
generation2026-10-19T19:03:34Z. Direct source reads and actual tests are evidence.
