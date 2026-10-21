# DPM++ 2M / CLIP contexts — 2026-10-20

Scope: image-editor adapter/UI only. Existing worktree changes are retained.
No StrictLib, Plakat fork, or shared model-runtime implementation changes.

## Contracts

- Real DPM-Solver++ 2M midpoint, Karras rho=7, VP epsilon prediction, final sigma=0,
  first-order initial/final update, explicit begin index for Img2Img and duplicate
  rounded timesteps. One prior x0 tensor retained. Solver arithmetic uses F32;
  output returns to denoiser dtype. Training schedule read from selected repo config.
- Plakat's named DpmppKarras delegates to UniPC without its corrector. This adapter
  uses its own solver and does not claim that CLI path implements the same method.
- Sampler selection persists in Request/PSD recipe/preferences and carries into
  Img2Img refinement. Missing sampler in old recipes = historical DDIM. Missing
  sampler in old UI preferences = new DPM++ default. API default remains DDIM.
- CLIP body tokens are split automatically into 75-token contexts with explicit
  BOS/EOS and encoder-specific padding; weights and BREAK boundaries retained.
  Positive/negative and both SDXL encoders padded to equal context count. No silent
  truncation; 16-context limit returns an error. Pooled SDXL embedding uses first
  context EOS. The reported 85-token prompt now forms two complete contexts.
- SDXL VAE executes in FP32, including model placement/cache key and pixel input;
  encoded latents return to the UNet dtype. Decode checks denoiser latents separately
  from VAE output. GPU CLIP/UNet remain FP16. VAE path/preferences are unchanged.

## Independent numerical reference

Reference implementation: Hugging Face Diffusers
[DPMSolverMultistepScheduler](https://github.com/huggingface/diffusers/blob/main/src/diffusers/schedulers/scheduling_dpmsolver_multistep.py).
The checked-in fixture was generated with installed Diffusers **0.35.1**, Torch CPU,
no model weights or network. Regenerate with `scripts/dpmpp_reference.py`.
Parameters: epsilon, scaled_linear beta=0.00085..0.012, 1000 training timesteps,
algorithm_type=dpmsolver++, solver_order=2, solver_type=midpoint,
use_karras_sigmas=True, final_sigmas_type=zero.

Cases: 1/4/8/20/50 steps from noise; 20 steps starting at index 15 and 150 starting
at 137 for Img2Img. Every timestep, sigma and intermediate output checked against
`tests/fixtures/dpmpp_diffusers.json`. Sigma absolute tolerance 2e-5; sample absolute
tolerance 2e-4. Tests do not claim bitwise agreement of FP16 CUDA or cross-engine RNG.

Additional tests cover preservation of 160 weighted content tokens across contexts,
EOS/weights/padding, empty negative and explicit BREAK, disabled tokenizer truncation,
explicit length-limit rejection, preference migration, legacy recipes, invalid
preferences, unsupported prediction and out-of-order solver calls.

## Verification commands

After sourcing `scripts/enter-cuda.ps1`, common features `demo,plakat-cuda,ollama`
and config overrides `profile.dev.package.plakat.opt-level=2`,
`profile.dev.package.sha2.opt-level=3`:

- `cargo +1.92.0 clippy -p image-editor --all-targets ... -- -D warnings`
- `cargo +1.92.0 test -p image-editor --all-targets ...`
- `cargo +1.92.0 build -p image-editor --bin image-editor-demo ...`

Native settings capture: `artifacts/dpm-settings.png`, DPM++ default visible,
isolated QA preference profile. This does not modify the user's profile.

## Hardware evidence

First 1024×1024 / 12-step run of the provided prompt and selected checkpoint with
explicit sdxlVAE_sdxlVAE failed the existing non-finite VAE output guard in FP16.
This exposed the need to upcast SDXL VAE computation; it was not recorded as a pass.
After upcasting VAE: **PASS**, selected checkpoint pornzillaHentai_v45, explicit
sdxlVAE_sdxlVAE, provided 85-token prompt, 1024×1024, CFG 7.5, seed 42,
12 DPM++ steps, raw generation without postprocessing. Completed in **54.5 s**.
Output `artifacts/dpm-long-prompt.png` inspected: valid rendered image; no black/
non-finite decode. This is a smoke check at 12 steps, not an image-level Invoke
quality comparison at the user's 50 steps.

Final strict Clippy PASS; **78 tests PASS** (43 library, 2 demo, 11 editing,
7 manual, 6 PSD, 3 runtime, 6 transforms); CUDA demo build PASS.
VAE dtype test verifies FP32 for SDXL with an FP16 denoiser. Native Img2Img **PASS**: same model/VAE/prompt, 1024×1024 source from the raw run,
4 scheduler steps, strength 0.6, begin index 2, two denoiser updates. Completed in
**42.2 s**, output `artifacts/dpm-img2img.png`. This exercises FP32 VAE encode,
F16 latent handoff, partial DPM++ schedule and FP32 VAE decode. Both runs retained
normal completion/queue-release assertions in native_smoke.

## Limits

No Invoke output/settings pair was supplied for an image-level comparison. Matching
solver equations and preserving the complete prompt do not establish full engine
parity: prompt weighting/pooling conventions, seed RNG, VAE and postprocessing may
still differ. Postprocessing is deliberately not run in the raw-generation check.

Graph project D-projects-StrictLib-mufoed generation 2026-10-19T19:03:34Z reports
missing freshness for these worktree paths. Graph/coverage checked; source reads,
independent fixtures, regression tests and native execution are the evidence.
