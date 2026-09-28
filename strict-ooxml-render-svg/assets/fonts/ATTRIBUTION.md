# Bundled font assets — attribution and licenses

These fonts are bundled **only** so the renderer (metrics + SVG rasterization)
produces deterministic, Word-comparable output without depending on system
fonts (ADR-0006, `STAGE-4-RENDER-FIDELITY.md` S4F.1/S4F.3). They are
**metric-compatible** substitutes for the proprietary fonts that Word documents
request; the proprietary originals (Calibri, Cambria, Arial, Times New Roman,
Courier New) are **not** bundled.

No font was modified; the OFL license text is included alongside each family.

## Files

| Family (bundled) | Metric-compatible with | License | Files |
|---|---|---|---|
| Carlito | Calibri | SIL Open Font License 1.1 (`carlito/OFL.txt`) | `carlito/Carlito-{Regular,Bold,Italic,BoldItalic}.ttf` |
| Caladea | Cambria | SIL Open Font License 1.1 (`caladea/OFL.txt`) | `caladea/Caladea-{Regular,Bold,Italic,BoldItalic}.ttf` |
| Arimo | Arial / Helvetica | SIL Open Font License 1.1 (`arimo/OFL.txt`) | `arimo/Arimo[wght].ttf`, `arimo/Arimo-Italic[wght].ttf` (variable `wght` 400/700) |
| Tinos | Times New Roman | SIL Open Font License 1.1 (`tinos/OFL.txt`) | `tinos/Tinos-{Regular,Bold,Italic,BoldItalic}.ttf` |
| Cousine | Courier New | SIL Open Font License 1.1 (`cousine/OFL.txt`) | `cousine/Cousine-{Regular,Bold,Italic,BoldItalic}.ttf` |

## Provenance

Downloaded from the **google/fonts** repository (github.com/google/fonts),
pinned per family. Arimo is bundled as the variable font; the renderer
instantiates `wght` 400/700 with `skrifa`.

| Path | Pinned commit |
|---|---|
| `ofl/carlito` | `3dd78844021e948ceb633d1dcee3f7885561b5d9` |
| `ofl/caladea` | `13c010f766052c13fa9cc8c656008cee55fb4775` |
| `ofl/arimo` | `903d46673260c1f4c7f7ef67f5190fb03eab5042` |
| `ofl/tinos` | `ba95515f1333efe9342c2ad988b9c2f6bef6dbad` |
| `ofl/cousine` | `ab75f7cf3e87c9ba8663f431f2ee4279c6406ee3` |

The Tinos `OFL.txt` is taken from the upstream `googlefonts/tinos` repository at
`3b4482a99b80ea5fc75f187b1be3120a3f5905b3` (google/fonts ships none for that
family).

## SHA-256

| File | SHA-256 |
|---|---|
| `arimo/Arimo[wght].ttf` | `e43898b143ec826ac8cb4034816458a7047fbe0836558de2a1f8c6223ae3e0ca` |
| `arimo/Arimo-Italic[wght].ttf` | `a80fc54fd0233c1dfe298577c4d00f5ae81d5bb83510975e473c47e699b7f4ed` |
| `caladea/Caladea-Regular.ttf` | `f1e899278b7b4491aba5b6a8253c4b04c050cc59b21865be5c37559a775153cd` |
| `caladea/Caladea-Bold.ttf` | `ae3cb2dcbc925809dd29d2a44e9802211cab66be541bacbfc9c08c74b27c3742` |
| `caladea/Caladea-Italic.ttf` | `4359a8e24f748b6447b1ff6d7a174febe70961d29f8bb8634b56dacd740a3deb` |
| `caladea/Caladea-BoldItalic.ttf` | `ccabaa7b7e2fdf253d2b1a5fa699dd8a3df8d835a9eb285ad82631a677eb76c0` |
| `carlito/Carlito-Regular.ttf` | `f6418f708baede9789daef5d458c0f53d2a888af9820e8062934e504fedc6595` |
| `carlito/Carlito-Bold.ttf` | `bb5d20f79b82599ec72983597437373a80f2d2085fa91fc144fd74e876a594db` |
| `carlito/Carlito-Italic.ttf` | `0b019225e58d702bfedcbd35c21696769f8ee115cb6343f84c2f240312450d1c` |
| `carlito/Carlito-BoldItalic.ttf` | `b32928186c119599e03ca6a1ffc680fdcb7fac95772f4b95d989cf6cd3861517` |
| `tinos/Tinos-Regular.ttf` | `60a0e8ef0c04dd5dd69ffe91025fa2ae5836cbd35600a82ba031977557e2cb61` |
| `tinos/Tinos-Bold.ttf` | `393269dbab8899f938db19783eca5eac92eb431f7ae0ab45b8349ca895f1a06b` |
| `tinos/Tinos-Italic.ttf` | `5942266ed398b155d7dc23e36833e7ec6be988f2439bdbeb8ef1bede808eaa91` |
| `tinos/Tinos-BoldItalic.ttf` | `a5de79f0fe863ea0954757acb3d47b3ccd0a930ce3dd5b97230cd3866790a06e` |
| `cousine/Cousine-Regular.ttf` | `1da22250675fc4c42fcf3a9736c44bc0570516105331443b663fd5cfbd1412fe` |
| `cousine/Cousine-Bold.ttf` | `17c8a7245156d2253531c9e529474937b09d9f641c5ae7695c5e33f22822eef4` |
| `cousine/Cousine-Italic.ttf` | `ea2a76ae3d0ece9cd59f0d30fdc08dd70e8f5f457beee5b0852a7b50c2286c7c` |
| `cousine/Cousine-BoldItalic.ttf` | `848e858726fee0ae27b754e4cd6a2755209bf1428a8c91f747696d58c33906c3` |

## License of the project

The font files keep their own licenses (above). The rest of the repository is
MIT OR Apache-2.0.
