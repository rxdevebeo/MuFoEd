# JPEG fixtures (AUD-81)

Minimal 8×8 solid-colour JPEGs used by the PDF writer's format tests:

| File | Layout |
|---|---|
| `jpeg-rgb-red.jpg` | SOF 3 components → `/DeviceRGB` |
| `jpeg-gray-128.jpg` | SOF 1 component → `/DeviceGray` |
| `jpeg-cmyk-adobe.jpg` | SOF 4 components + Adobe APP14 `ColorTransform=2` → `/DeviceCMYK` with `/Decode [1 0 1 0 1 0 1 0]` |

Regenerate with:

```text
cargo run -p xtool -- gen-jpeg
```
