# R09 ACCEPTANCE — Browser acceptance F07/F20

Date: 2026-10-05

## Identifiers

| Field | Value |
|---|---|
| Code HEAD | `fa2f41b4455977588bbb980daf219debe6629bf9` (dirty tree; R01–R08 on master) |
| Dirty-tree fingerprint (`git status --porcelain` SHA-256) | `98c75fed6de2918289b4da4e943f9738b6d0a55e058ab1c0eb26c1a941538d87` |
| Toolchain | `cargo +1.92.0 --locked`, Node (CDP), Edge headless |
| Pinned browser | `C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe` · Edg/154.0.4258.53 |
| Carlito resource_hash (R05) | `f6418f708baede9789daef5d458c0f53d2a888af9820e8062934e504fedc6595` |
| Result | **PASS** |

Source SHA-256 (post-fix): `target/audit-rework-2026-10-05/R09/green/source-hashes.txt`.

## Bug (RED)

`f20_successful_view_exposes_normalization_loss` only did TCP GET + string search on HTML/JSON. It did not execute JavaScript, interact with the loss UI, or wait for fonts. F07 SVG tests checked `@font-face` markup only. UI did not publish `pipeline.sidecar` onto the DOM.

Probe: `target/audit-rework-2026-10-05/R09/red/red_probe.txt` (`STATUS=red`).

## Fix

1. **`xtool/browser-gate/`** — pinned Chromium-family CDP harness (`run.mjs` + `run.py`). Exit `3` = BLOCKED when browser/node missing; never PASS on static HTML alone. App crates do not depend on the tool.
2. **`strict-ooxml-view/src/ui.rs`** — `data-outcome` / `data-sidecar` on `#losses`; Enter/Space keyboard toggle; `user-select: text` on rendered pages; sidecar shown in the summary label.
3. **`tests/r09_browser.rs`** — live browser witnesses for F20 matrix + F07 face load; inverse proves static `/` HTML has empty losses until JS.
4. **Matrix/manifest** — `F20-matrix-browser-js`, `F20-inverse-static-html`, `F07-browser-face-load` → `measured`. Existing TCP API test kept.

## GREEN

```
cargo +1.92.0 test -p strict-ooxml-view --locked --test r09_browser -- --nocapture
cargo +1.92.0 test -p strict-ooxml-view --locked --test f20_view --lib
cargo +1.92.0 test -p strict-ooxml-view --locked --bin strict-ooxml-view
cargo +1.92.0 clippy -p strict-ooxml-view --locked --all-targets --no-deps -- -D warnings
```

| Check | Result |
|---|---|
| `f20_browser_dom_after_js_exposes_loss_matrix` | pass (degraded+rejected, clean+not_run, matched+escape+6 issues, failed) |
| `f07_browser_loads_bundled_face_bytes_and_hash` | pass (`carlitoLoaded=true`, bytes hash = R05) |
| `inverse_static_html_shell_has_empty_losses_until_js` | pass |
| `f20_successful_view_exposes_normalization_loss` (API) | pass (unchanged) |
| UI/bin unit tests | 35/35 pass |
| clippy `-D warnings` | pass |

Reports: `target/audit-rework-2026-10-05/R09/green/f20-browser-report.json`, `f07-browser-report.json`.

## Inverse

Static TCP HTML shell must not already contain live loss fill / sidecar text. Log: `target/audit-rework-2026-10-05/R09/inverse/static-html-inverse.log`.

## Limits

- Remaining F07 theme/tab/missing-glyph matrix row stays **blocked** (not part of the browser gate).
- Without Edge/Chrome/Chromium or Node, the gate exits **BLOCKED** (not PASS).
- Commit/push not performed (owner permission required).
