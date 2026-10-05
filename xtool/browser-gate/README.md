# Browser gate (R09 / F07 / F20)

Pinned Chromium-family acceptance for:

- **F20** — viewer loss UI after live JS (DOM + API): clean / degraded / failed /
  `not_run`, keyboard expand, detail escaping, many issues, matched vs rejected sidecar
- **F07** — actual `@font-face` load in the viewer: bytes SHA-256 equals the R05
  Carlito `resource_hash`, selectable text, no reliance on installed fonts

## Tools

- `run.py` — starts `strict-ooxml-view` against a fixtures directory and invokes `run.mjs`
- `run.mjs` — CDP client against local Edge/Chrome (`STRICTLIB_BROWSER` or well-known paths)

App crates do **not** depend on this tool. Missing browser/node → exit **3** (`BLOCKED`),
never a silent pass on static HTML.

## Exit codes

| Code | Meaning |
|---:|---|
| 0 | PASS |
| 1 | FAIL |
| 3 | BLOCKED (runtime missing) |

## Invocation

Prefer the Rust witnesses (they build fixtures and the viewer):

```
cargo +1.92.0 test -p strict-ooxml-view --locked --test r09_browser -- --nocapture
```
