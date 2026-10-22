# Source attribution

UNet building blocks derive from vulogov/plakat revision ce0d67a45bc9b5f742c3772aef85afe0d15890f6 (Unlicense, LICENSE), which adapts Hugging Face Candle 0.10.2 (Apache-2.0, LICENSE-CANDLE). Copyright Hugging Face and the Candle contributors.

Only the three building-block modules are included; the Plakat crate remains the pinned Git dependency. Local differences: import paths, vendored lint boundaries, correct head-slice concatenation, final partial slice, stable softmax kernel matching the unsliced reference, and query-row chunks for large token grids. Every query chunk attends to all keys with the full softmax; no approximation is used. The public adapter/configuration and tests live in mod.rs. No checkpoint layout, layer topology or SDXL conditioning is changed. UPSTREAM.json records original source hashes.
