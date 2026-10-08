# hostq - the host job executor for StrictLib

`hostq.ps1` is a tool, not an agent. It watches a file queue under the repo
root, validates each job against a fixed whitelist, runs the allowed kind,
and writes a report. Anything outside the whitelist is rejected without
launching a process.

It is a port of David's `tools/hostq` (work order OPS.1) to the
`strict-ooxml` workspace: the queue, the lock, the report and the invariants
are the same; the kinds are StrictLib's gates. The VM submits jobs over the
shared folder; the host runs gates, pixel tests, the XSD/OPC/census gates and
corpus scans on an exact SHA, and makes narrow commits, without hand-copied
logs.

## Run

```
tools\hostq\hostq.bat [-AllowPush]
```

`hostq.bat` needs no arguments: the repo defaults to the copy two levels
above `tools\hostq` (`D:\projects\StrictLib` for the main copy), or to
`%HOSTQ_REPO%` if that is set, or to an explicit first argument. Flags are
forwarded to `hostq.ps1`, so the equivalent direct call is:

```
powershell -ExecutionPolicy Bypass -File tools\hostq\hostq.ps1 -Repo D:\projects\StrictLib [-AllowPush]
```

`hostq.bat` sets `CARGO_BUILD_JOBS=4` unless it is already set: the host ran
out of memory linking test binaries in parallel. Set your own value before
launching to override it (`set CARGO_BUILD_JOBS=2`). The direct
`powershell -File` call does not set it.

It runs in the foreground. `Ctrl+C` stops after the current job: the key is
read between jobs, so a running job is never cut in half (close the window
to abort one; per-kind timeouts kill the whole process tree anyway). There
is no service, scheduler or autostart.

`-SelfTest` runs the falsifiers against a temporary repository and a
temporary queue (the real main copy is never touched) and prints a check
table:

```
tools\hostq\hostq.bat -SelfTest
```

The self-test needs `git`, `rustup` with the `1.92.0` toolchain (rustfmt
included) and `python` on `PATH`, because it runs a real `ping`, a real
`cargo +1.92.0 fmt`/`test` on a one-file crate, and a `pixels` job that
fails fast on the missing packages. It never builds the workspace.

Every cargo call uses `cargo +1.92.0` (CI pins 1.92.0):

```
rustup toolchain install 1.92.0 --component rustfmt,clippy
cargo install cargo-deny --locked
pip install -r xtool\xsd-gate\requirements.txt
```

## Queue layout

Everything lives in `<repo>\StrictLib-hostq\` (ignored by `/StrictLib-hostq/`):

| dir | contents |
|---|---|
| `inbox\` | jobs waiting to be taken (`<id>.json`) |
| `running\` | the one job being executed |
| `done\` | finished jobs |
| `out\` | reports (`<id>.json`), full logs (`<id>.log`), artifacts (`<id>\`) |
| `wt\` | the reusable detached worktree for `gate`/`pixels`/`xsd`/`corpus-scan` |
| `target\` | `CARGO_TARGET_DIR` for every build the executor starts |
| `current` | the executor's own current job id (absent when idle) |
| `heartbeat.json` | pid, UTC time, current job - rewritten every 30 s |
| `lock` | held open with `FileShare.None` for the process lifetime (pid and start time); a second launch exits 3 |

One executor per queue: the first launch holds `lock`; a second prints
`hostq: already running (pid N)` and exits with code 3, leaving the first
running. Every text file is UTF-8 **without BOM**.

## Job protocol

A job is `inbox\<id>.json`. The writer writes `<id>.json.tmp` and then
renames it: the executor only ever sees complete `*.json` files. `id`
matches `^\d{8}-\d{4}-[a-z0-9-]{1,32}$` (for example `20261007-1200-ping`),
and the `id` field, when present, must equal the file stem.

Jobs are taken one at a time in ascending `id` order: `inbox` -> `running`,
then `running` -> `done` when finished.

To place a job from the VM over the shared folder, write the temp file and
rename it into place (a rename is atomic within a directory):

```python
import json, os

inbox = "/home/mint/projects/StrictLib/StrictLib-hostq/inbox"
job = {"id": "20261007-1200-gate", "kind": "gate", "sha": "9acf0a8"}

tmp = os.path.join(inbox, job["id"] + ".json.tmp")
with open(tmp, "w", encoding="utf-8", newline="\n") as f:
    json.dump(job, f)
os.rename(tmp, os.path.join(inbox, job["id"] + ".json"))
```

Then wait for `out/<id>.json` (it appears last, after the log and the
artifacts) and read it; `heartbeat.json` says whether the executor is alive
and which job it is on.

### Whitelist (v1)

Any other `kind`, any extra field, or any value outside the template is
`rejected`. Kind, field names and enumerated values are case-sensitive.
`sha` must match `^[0-9a-f]{7,40}$` and resolve to a commit. `paths[]` are
repo-relative, without `..`, inside the repo.

| kind | fields | what it does | timeout |
|---|---|---|---|
| `ping` | `id`, `kind` | `git --version`, `rustup show active-toolchain`, `cargo +1.92.0 --version`, `rustc +1.92.0 --version`, the 1.92.0 components, `python --version`, `cargo deny --version` if installed, free disk, repo path, `CARGO_BUILD_JOBS`, and for each corpus dir whether it exists and its file count; `failed` only if a required probe (git, rustup, cargo, rustc, python) fails | 1 min |
| `git-info` | `id`, `kind`, `rev`, `range?` | `rev-parse`, `log --oneline -n 50 <range>`, `diff --stat <range>`, `status --porcelain` of the main copy; read-only | 2 min |
| `gate` | `id`, `kind`, `sha`, `steps?[]` | in `wt`, after copying the corpora: `fmt --all -- --check`; `clippy --workspace --all-targets --all-features -- -D warnings`; `test --workspace --all-features --no-fail-fast`; `deny check`; `doc --workspace --no-deps`. **All** steps run, not to the first failure. `steps` is a subset of `fmt`,`clippy`,`test`,`deny`,`doc` (non-empty, no duplicates; run in the canonical order above). `out\<id>\gates.txt` carries every `test result:` line, the summed passed/failed/ignored, the failing test names and the failing targets, read from the step's full output (never the 40-line tail) | 120 min |
| `pixels` | `id`, `kind`, `sha` | in `wt`: `test -p strict-ooxml-render-svg --all-features --test ssim` and `test -p strict-ooxml-pdf --features raster --test pdf_pixels`; both run; each output is an artifact | 60 min |
| `xsd` | `id`, `kind`, `sha`, `gate` | `gate` = `xsd`/`opc`/`census`. In `wt`: `build --release -p strict-ooxml-cli`, then: **xsd** - write every `strict-ooxml-core/tests/strict/*.docx` with that binary into `out\<id>\written\`, `python xtool/xsd-gate/xsd_gate.py --written <dir> --quiet-messages`; **opc** - the same write, `python xtool/xsd-gate/opc_gate.py --written <dir>`; **census** - copies `tests/docx` and `testdata/CC0_DOCX` first, then `python xtool/xsd-gate/census_gate.py --cli <binary> --quiet-messages --write-reports out\<id>\census-reports --inventory-out out\<id>\census-inventory.json`. The gate's full output (`<gate>-gate.txt`) and the census files are artifacts | 120 min |
| `wps` | `id`, `kind`, `sha` | the P1 WPS geometry protocol on Clio: copies `strict-ooxml-core/tests/docx`, renders pages 54-104 with `--wps-times` and again with defaults (a render that writes pages counts as done although the CLI exits 1 for Clio's unsupported mechanisms), and after each render runs `xtool/wps-gate/wps_ledger.py` and `wps_p1_gate_selftest.py` (the host Python needs `pip install -r xtool/wps-gate/requirements.txt`); `wps-ledger-<variant>.json` and the script outputs are the artifacts | 60 min |
| `bench` | `id`, `kind`, `sha`, `bench` | `bench` = `editing`/`opc`/`render`/`wml_parse`/`xml_scan`; `cargo bench -p <its package> --bench <bench> -- --noplot`; criterion's stdout is `bench-<bench>.txt` | 90 min |
| `corpus-scan` | `id`, `kind`, `sha`, `set` | `set` = `CC0`/`CC0_DOCX`/`CC0_DOCX_1`; copies only `testdata/<set>`, then `run -p strict-ooxml --features write,svg --example corpus_scan --release -- testdata/<set>`; its stdout (OK/FAIL rows) is `corpus-scan-<set>.tsv`, its stderr (summary by stage) `corpus-scan-<set>-summary.txt` | 120 min |
| `commit` | `id`, `kind`, `branch`, `paths[]`, `message` | in the **main** copy: the current branch must equal `branch`, else `rejected`; the exact paths are `git add`ed first (new files must be committable), then `git commit --only -F <message> -- <paths>`; `message` is a repo-relative path to the message file | 10 min |
| `push` | `id`, `kind`, `branch` | only with `-AllowPush`; only `task/*`, `docs/*` and `codex/*`; `master` and `main` are rejected; never `--force`. `branch` must be a plain source name - a `:` would be a refspec (`task/x:master`) and is rejected | 10 min |

Every `cargo` above is `cargo +1.92.0` with `CARGO_TARGET_DIR=<queue>\target`
and `CARGO_TERM_COLOR=never`. `merge`, `reset`, `rebase` and arbitrary
commands are not in the whitelist and never will be in v1.

Example of each kind:

```json
{"id":"20261007-1200-ping","kind":"ping"}
{"id":"20261007-1201-info","kind":"git-info","rev":"HEAD","range":"HEAD~2..HEAD"}
{"id":"20261007-1202-gate","kind":"gate","sha":"9acf0a8"}
{"id":"20261007-1203-gate-fast","kind":"gate","sha":"9acf0a8","steps":["fmt","clippy"]}
{"id":"20261007-1204-pixels","kind":"pixels","sha":"9acf0a8"}
{"id":"20261007-1205-xsd","kind":"xsd","sha":"9acf0a8","gate":"xsd"}
{"id":"20261007-1206-opc","kind":"xsd","sha":"9acf0a8","gate":"opc"}
{"id":"20261007-1207-census","kind":"xsd","sha":"9acf0a8","gate":"census"}
{"id":"20261007-1208-scan","kind":"corpus-scan","sha":"9acf0a8","set":"CC0_DOCX"}
{"id":"20261007-1209-commit","kind":"commit","branch":"task/p07","paths":["docs/reviews/NOTE.md"],"message":"docs/reviews/NOTE.md"}
{"id":"20261007-1210-push","kind":"push","branch":"task/p07"}
```

### How this maps to ci.yml

| hostq | ci.yml step | difference |
|---|---|---|
| gate `fmt` | `Format` | none |
| gate `clippy` | `Clippy` | none |
| gate `test` | `Test` | `--no-fail-fast`, so every failing target is listed |
| gate `deny` | `deny` job (`cargo-deny-action`) | runs `cargo deny check` locally |
| gate `doc` | `Docs` | none (CI sets no `RUSTDOCFLAGS`) |
| `pixels` | `Render fidelity`, `PDF render fidelity` | none |
| `xsd` `xsd`/`opc` | `xsd-gate` job: `Build the writer`, `Write the corpus`, `XSD gate`, `OPC gate` | one gate run per job, not the reproducibility rerun or the selftests |
| `xsd` `census` | not in CI (waiver `CENSUS-LOCAL`: the corpus is local) | - |

CI's `CARGO_TERM_COLOR=always` is replaced by `never` so logs and the
`test result:` parsing see no ANSI escapes. CI steps not listed (the default
features check, the feature matrices, coverage, fuzz, the xtool lints) are
not hostq kinds.

### Report

`out\<id>.json` is written last, through a temp file and a rename. It carries:

- `id`, `kind`, `status` (`ok` | `failed` | `rejected` | `timeout` | `error`),
  `reason`;
- `started` / `finished` (UTC, ISO-8601);
- `sha` - the resolved full commit id for `gate`/`pixels`/`xsd`/`corpus-scan`,
  empty otherwise;
- `steps[]` - `name`, `cmd`, `exit`, `seconds`, `tail` (last 40 lines);
  corpus copies appear as `copy <dir>` steps;
- `artifacts[]` - `path` (relative to the queue dir), `sha256`;
- `main_before` / `main_after` - `head`, `branch`, `status_sha256` (SHA-256 of
  `git status --porcelain`) and `corpus_sha256` (SHA-256 of the listing -
  path, size, mtime - of `strict-ooxml-core/tests/docx`, `testdata/CC0`,
  `testdata/CC0_DOCX`, `testdata/CC0_DOCX_1`);
- `main_changed` - booleans `head`, `branch`, `status`, `corpus` computed
  from `main_before`/`main_after`. It is a record only: the submitter works in
  the main copy too, so a change there never fails a job.

The full stdout+stderr of every step is in `out\<id>.log`. Artifacts are in
`out\<id>\`.

## Invariants

- Rejection never launches a process: an unknown kind, an extra field, a bad
  id, a sha that does not resolve, a value outside its set.
- A change to the main copy while a job runs is **recorded, not fatal**: the
  executor cannot distinguish its own write from the submitter's, so
  `main_changed` is the only trace and the job's status is unaffected. The
  submitter may commit in the main copy during a job.
- The one change that stops a job is to the source the job reads: the
  corpora are gitignored, so a worktree does not have them, and `gate`,
  `xsd census` and `corpus-scan` copy them from the main copy (robocopy).
  Each directory is inventoried before and after its copy, and a difference
  gives `status = error`, `reason = "source changed during copy: N file(s)"`,
  the first 20 paths in the log, and no build or run steps. A copy that
  takes over 10 minutes is an `error` too. The executor continues with the
  next job.
- The `wt` worktree is reused: `checkout --detach --force <sha>` then
  `clean -ffdx`, then the corpus copy (so `clean` removes the previous job's
  copy and this job's copy is fresh); `target` lives outside it.
- Only `commit` and `push` touch the main copy's git state; everything else
  reads it or works in `wt`.
- The executor publishes no numbers. The report is an artifact; only the
  acceptance role writes a figure into the record.

## Troubleshooting

- **"stale job file(s) in running\"** on startup: the previous executor
  crashed or its window was closed mid-job. The job is not retried and the
  file is not deleted. Read `out\<id>.log` (the report may be missing), then
  delete `running\<id>.json`, or move it back to `inbox\` to run it again.
- **`LNK1285` / a corrupt `.pdb`** after a killed build (timeout or closed
  window): delete the files of that one crate under
  `StrictLib-hostq\target\debug\deps\<name>-<hash>.*` (or `release\deps\`)
  named in the error, and resubmit. Deleting the whole `target\` also works
  and costs a full rebuild.
- **Out of memory** while compiling or linking tests: lower
  `CARGO_BUILD_JOBS` (`set CARGO_BUILD_JOBS=2` before `hostq.bat`), restart
  the executor, resubmit.
- **`hostq: already running (pid N)`**, exit 3: one executor per queue. Find
  that window, or check that pid; `heartbeat.json` says when it was last
  alive. The lock is released when the process exits.
- **`deny` fails with "no such command"**: `cargo install cargo-deny`.
- **`xsd` fails before validating**: `pip install -r xtool\xsd-gate\requirements.txt`;
  the first run downloads the ECMA schema set into the user cache
  (`STRICT_XSD_DIR` points it at an offline copy, see `xtool/xsd-gate/README.md`).
- **`corpus-scan` errors "absent in the main copy"**: the set is gitignored
  and must exist under `testdata\` in the main copy; `ping` lists which
  corpora are present.
