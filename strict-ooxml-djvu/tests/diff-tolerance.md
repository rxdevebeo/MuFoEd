# IW44 render tolerances (SC-D7)

Exactness (`SSIM = 1.0`) is claimed only for bitonal pages, listed in
`docs/djvu-validation/render-bitonal.json`. This file records the measured,
honest tolerance for IW44 pages and why it is not exact.

Cause: our IW44 planes are upscaled from their reduced resolution by the nearest
pixel, while `ddjvu` interpolates; the wavelet reconstruction (gamma) differs
too. The difference concentrates on smooth colour areas and edges, not on the
bilevel layer.

Measured 2026-10-09 against `djvulibre-bin 3.5.28-2ubuntu0.24.04.2`. Sample:
the 14 smallest corpus files, every page of each (69 page comparisons; 62 have a
composite on both sides), listed in
`docs/djvu-validation/render-sample.json`. Composite mode:

| Metric | Worst |
| --- | --- |
| mean channel error | 1.531 |
| max channel error | 244 |
| PSNR | 31.31 dB |

The gate (`docs/djvu-validation/iw44-tolerance.json`) is the measured envelope
with headroom: mean channel error ≤ 2.0 and PSNR ≥ 30 dB, applied to the sample
manifest. It is a regression floor, not a fidelity claim: a change that lowers
PSNR below 30 dB or raises the mean above 2.0 on any compared page fails.
Raising the floor is a code change (better IW44 upscaling), never a tolerance
edit.
