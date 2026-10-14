#!/usr/bin/env python3
"""WPS reference QA: capture WPS references for every corpus and measure our
render against them. See README.md.

    python xtool/wps-qa/wps_qa.py capture [--corpus NAME ...]
    python xtool/wps-qa/wps_qa.py render  [--cli PATH] [--label L]
    python xtool/wps-qa/wps_qa.py compare [--label L]
    python xtool/wps-qa/wps_qa.py report  [--label L] [--baseline L0]
    python xtool/wps-qa/wps_qa.py run     (all four in order)

This is a QA instrument, not a gate: it publishes measurements, never a
pass/fail exit code for the render.
"""

from __future__ import annotations

import argparse
import concurrent.futures as cf
import json
import os
import shutil
import subprocess
import sys
import time
import traceback
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import wpsqa_common as common  # noqa: E402
from wpsqa_common import Doc, read_json, sha256_file, write_json  # noqa: E402

WORKER = common.HERE / "wps_worker.py"


# ---------------------------------------------------------------------------
# capture: WPS -> reference PDF
# ---------------------------------------------------------------------------


def _kill_pids(pids: list[int]) -> list[int]:
    import psutil

    killed = []
    for pid in pids:
        try:
            proc = psutil.Process(pid)
            if (proc.name() or "").lower() != "wps.exe":
                continue
            for child in proc.children(recursive=True):
                child.kill()
            proc.kill()
            killed.append(pid)
        except psutil.Error:
            pass
    return killed


def _kill_tree(pid: int) -> None:
    import psutil

    try:
        proc = psutil.Process(pid)
        for child in proc.children(recursive=True):
            child.kill()
        proc.kill()
    except psutil.Error:
        pass


def capture_one(doc: Doc, store: Path, timeout: int) -> dict:
    ref_dir = store / "refs" / doc.corpus
    pdf = ref_dir / f"{doc.key}.pdf"
    meta_path = ref_dir / f"{doc.key}.json"
    tmp = store / "tmp" / "capture"
    shutil.rmtree(tmp, ignore_errors=True)
    tmp.mkdir(parents=True)
    # WPS reads a private copy under an ASCII name: the corpus is never opened
    # by WPS, and no path quirk of a corpus file reaches COM. The extension
    # stays, so WPS opens an RTF as RTF.
    src = tmp / ("in" + doc.path.suffix.lower())
    shutil.copyfile(doc.path, src)
    out = tmp / "out.pdf"
    pidfile = tmp / "pids.json"
    meta: dict = {
        "corpus": doc.corpus,
        "key": doc.key,
        "source": str(doc.path),
        "source_sha256": sha256_file(doc.path),
        "captured": time.strftime("%Y-%m-%dT%H:%M:%S"),
    }
    started = time.time()
    proc = subprocess.Popen(
        [sys.executable, "-I", str(WORKER), str(src), str(out), str(pidfile)],
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        encoding="utf-8",
        errors="replace",
        creationflags=getattr(subprocess, "CREATE_NO_WINDOW", 0),
    )
    try:
        stdout, stderr = proc.communicate(timeout=timeout)
        result = {}
        for line in stdout.splitlines()[::-1]:
            if line.startswith("{"):
                result = json.loads(line)
                break
        meta.update({k: v for k, v in result.items() if k not in ("ok", "wps_pids")})
        meta["status"] = "ok" if result.get("ok") else "failed"
        if not result:
            meta["error"] = "worker printed no result: " + common.tail(stderr, 5)
    except subprocess.TimeoutExpired:
        _kill_tree(proc.pid)
        proc.communicate()
        meta["status"] = "timeout"
        meta["error"] = f"no export within {timeout}s"
    finally:
        pids = read_json(pidfile) or []
        # A WPS process that outlived its worker (or a hung one) is ours to end.
        time.sleep(0.5)
        meta["killed_wps_pids"] = _kill_pids(pids)
    meta["seconds_total"] = round(time.time() - started, 2)
    ref_dir.mkdir(parents=True, exist_ok=True)
    if meta["status"] == "ok":
        try:
            import pypdfium2 as pdfium

            d = pdfium.PdfDocument(str(out))
            meta["pages"] = len(d)
            d.close()
            os.replace(out, pdf)
            meta["pdf_sha256"] = sha256_file(pdf)
        except Exception as exc:  # noqa: BLE001
            meta["status"] = "failed"
            meta["error"] = f"unreadable PDF: {exc}"
    if meta["status"] != "ok" and pdf.exists():
        pdf.unlink()
    write_json(meta_path, meta)
    return meta


def cmd_capture(args) -> int:
    store = args.store
    docs = list(common.iter_docs(args.data_root, args.corpus))
    if args.limit:
        docs = docs[: args.limit]
    todo = []
    for doc in docs:
        meta = read_json(store / "refs" / doc.corpus / f"{doc.key}.json")
        fresh = meta and meta.get("source_sha256") == sha256_file(doc.path)
        if fresh and not args.force and (meta["status"] == "ok" or not args.retry_failed):
            continue
        todo.append(doc)
    print(f"capture: {len(docs)} documents, {len(todo)} to capture", flush=True)
    counts: dict[str, int] = {}
    for n, doc in enumerate(todo, 1):
        meta = capture_one(doc, store, args.timeout)
        counts[meta["status"]] = counts.get(meta["status"], 0) + 1
        detail = f"{meta.get('pages', '-')}p" if meta["status"] == "ok" else meta.get("error", "")[:120]
        print(f"[{n}/{len(todo)}] {meta['status']:<7} {meta['seconds_total']:>6.1f}s {doc.ident}  {detail}", flush=True)
    print(f"capture: done {counts}", flush=True)
    return 0


# ---------------------------------------------------------------------------
# render: our CLI -> PDF
# ---------------------------------------------------------------------------


def default_cli() -> Path:
    exe = "strict-ooxml.exe" if os.name == "nt" else "strict-ooxml"
    return common.main_copy_root() / "target" / "release" / exe


def resolve_label(args) -> str:
    if args.label:
        return args.label
    cli = Path(args.cli)
    if not cli.is_file():
        raise SystemExit(f"wpsqa: no CLI at {cli}; build it (cargo build --release -p strict-ooxml-cli) or pass --cli/--label")
    return "bin-" + sha256_file(cli)[:10]


# The corpus profile of the RTF importer (docs/rtf/ACCEPTANCE.md): the RTF
# corpora are read the way WPS reads them. Exit 0 = Clean, 1 = Degraded.
RTF_IMPORT_FLAGS = ("--wps-text", "--allow-unsigned-unicode", "--drop-nul")
RTF_STATUS = {0: "Clean", 1: "Degraded"}


def _run_cli(argv: list[str], timeout: int) -> tuple[int | None, str]:
    """Exit code (None on timeout) and stderr of one CLI call."""
    try:
        proc = subprocess.run(
            argv,
            capture_output=True,
            text=True,
            encoding="utf-8",
            errors="replace",
            timeout=timeout,
        )
    except subprocess.TimeoutExpired:
        return None, ""
    return proc.returncode, proc.stderr


def _import_rtf(doc: Doc, cli: Path, docx: Path, timeout: int, meta: dict) -> bool:
    """Imports the RTF source to `docx`, recording the import in `meta`;
    whether there is a DOCX to render."""
    report = docx.with_name(docx.name + ".rtf-report.json")
    for stale in (docx, report):
        if stale.exists():
            stale.unlink()
    code, stderr = _run_cli(
        [str(cli), "import-rtf", str(doc.path), "--out", str(docx), "--report", str(report), *RTF_IMPORT_FLAGS],
        timeout,
    )
    meta["import_exit"] = code
    meta["import_status"] = "Timeout" if code is None else RTF_STATUS.get(code, "Failed")
    facts = read_json(report) or {}
    meta["import_losses"] = (facts.get("import") or {}).get("loss_count")
    meta["writer_losses"] = (facts.get("writer") or {}).get("loss_count")
    if code not in RTF_STATUS or not docx.is_file():
        meta["stderr_tail"] = common.tail(stderr, 12)
        return False
    return True


def render_one(doc: Doc, cli: Path, out_dir: Path, timeout: int) -> dict:
    pdf = out_dir / doc.corpus / f"{doc.key}.pdf"
    meta_path = pdf.with_suffix(".json")
    pdf.parent.mkdir(parents=True, exist_ok=True)
    tmp_pdf = pdf.with_name(pdf.stem + ".part.pdf")
    if tmp_pdf.exists():
        tmp_pdf.unlink()
    meta = {"corpus": doc.corpus, "key": doc.key, "source_sha256": sha256_file(doc.path)}
    started = time.time()
    # An RTF is imported to Strict DOCX first, which is kept beside the PDF;
    # a DOCX of a corpus is read through the transitional normalizer.
    source = doc.path
    extra = ["--transitional"]
    ready = True
    if doc.is_rtf:
        source = pdf.with_suffix(".docx")
        extra = []
        ready = _import_rtf(doc, cli, source, timeout, meta)
    if ready:
        code, stderr = _run_cli([str(cli), "to-pdf", str(source), *extra, "--out", str(tmp_pdf)], timeout)
        meta["exit"] = code
        meta["stderr_tail"] = common.tail(stderr, 12)
        # Exit 1 = rendered with reported losses; the PDF exists and counts.
        if code is None:
            meta["status"] = "timeout"
        else:
            meta["status"] = "ok" if code in (0, 1) and tmp_pdf.is_file() and tmp_pdf.stat().st_size > 0 else "failed"
    else:
        meta["status"] = "timeout" if meta.get("import_status") == "Timeout" else "import-failed"
    meta["seconds"] = round(time.time() - started, 2)
    if meta["status"] == "ok":
        os.replace(tmp_pdf, pdf)
        meta["pdf_sha256"] = sha256_file(pdf)
    else:
        if tmp_pdf.exists():
            tmp_pdf.unlink()
        if pdf.exists():
            pdf.unlink()
    write_json(meta_path, meta)
    return meta


def cmd_render(args) -> int:
    label = resolve_label(args)
    cli = Path(args.cli)
    out_dir = args.store / "ours" / label
    write_json(
        out_dir / "build.json",
        {"label": label, "cli": str(cli), "cli_sha256": sha256_file(cli) if cli.is_file() else None, "when": time.strftime("%Y-%m-%dT%H:%M:%S")},
    )
    docs = list(common.iter_docs(args.data_root, args.corpus))
    if args.limit:
        docs = docs[: args.limit]
    todo = []
    for doc in docs:
        meta = read_json(out_dir / doc.corpus / f"{doc.key}.json")
        if meta and not args.force and meta.get("source_sha256") == sha256_file(doc.path):
            continue
        todo.append(doc)
    print(f"render[{label}]: {len(docs)} documents, {len(todo)} to render", flush=True)
    counts: dict[str, int] = {}
    with cf.ThreadPoolExecutor(max_workers=args.jobs) as pool:
        futures = {pool.submit(render_one, d, cli, out_dir, args.timeout): d for d in todo}
        for n, fut in enumerate(cf.as_completed(futures), 1):
            meta = fut.result()
            counts[meta["status"]] = counts.get(meta["status"], 0) + 1
            if meta["status"] != "ok" or n % 25 == 0:
                print(f"[{n}/{len(todo)}] {meta['status']:<7} {futures[fut].ident}", flush=True)
    print(f"render[{label}]: done {counts}", flush=True)
    return 0


# ---------------------------------------------------------------------------
# compare: metrics per document
# ---------------------------------------------------------------------------


def compare_one(ref_pdf: str, our_pdf: str, img_path: str) -> dict:
    import numpy as np
    from PIL import Image

    import metrics

    started = time.time()
    try:
        ref = metrics.read_pdf(ref_pdf)
        ours = metrics.read_pdf(our_pdf)
        result = metrics.compare(ref, ours)
        worst = result.get("raster", {}).get("worst_page")
        if worst:
            p, q = worst["ref_page"] - 1, worst["our_page"] - 1
            img = metrics.diff_image(metrics.rasterize(ref_pdf, p, metrics.IMAGE_DPI), metrics.rasterize(our_pdf, q, metrics.IMAGE_DPI))
            Path(img_path).parent.mkdir(parents=True, exist_ok=True)
            Image.fromarray((np.clip(img, 0, 1) * 255).astype(np.uint8)).save(img_path, optimize=True)
            result["image"] = True
        result["status"] = "ok"
    except Exception as exc:  # noqa: BLE001
        result = {"status": "error", "error": f"{type(exc).__name__}: {exc}", "trace": traceback.format_exc()[-2000:]}
    result["seconds"] = round(time.time() - started, 2)
    return result


def cmd_compare(args) -> int:
    label = resolve_label(args)
    ours_dir = args.store / "ours" / label
    cmp_dir = args.store / "cmp" / label
    docs = list(common.iter_docs(args.data_root, args.corpus))
    if args.limit:
        docs = docs[: args.limit]
    todo = []
    for doc in docs:
        ref_meta = read_json(args.store / "refs" / doc.corpus / f"{doc.key}.json") or {}
        our_meta = read_json(ours_dir / doc.corpus / f"{doc.key}.json") or {}
        if ref_meta.get("status") != "ok" or our_meta.get("status") != "ok":
            continue
        out = cmp_dir / doc.corpus / f"{doc.key}.json"
        prev = read_json(out)
        if (
            prev
            and not args.force
            and prev.get("status") == "ok"
            and prev.get("ref_pdf_sha256") == ref_meta.get("pdf_sha256")
            and prev.get("our_pdf_sha256") == our_meta.get("pdf_sha256")
        ):
            continue
        todo.append((doc, ref_meta, our_meta))
    print(f"compare[{label}]: {len(todo)} to compare", flush=True)
    with cf.ProcessPoolExecutor(max_workers=args.jobs) as pool:
        futures = {}
        for doc, ref_meta, our_meta in todo:
            ref_pdf = args.store / "refs" / doc.corpus / f"{doc.key}.pdf"
            our_pdf = ours_dir / doc.corpus / f"{doc.key}.pdf"
            img = cmp_dir / "img" / doc.corpus / f"{doc.key}.png"
            fut = pool.submit(compare_one, str(ref_pdf), str(our_pdf), str(img))
            futures[fut] = (doc, ref_meta, our_meta)
        for n, fut in enumerate(cf.as_completed(futures), 1):
            doc, ref_meta, our_meta = futures[fut]
            result = fut.result()
            result.update(
                {
                    "corpus": doc.corpus,
                    "key": doc.key,
                    "ref_pdf_sha256": ref_meta.get("pdf_sha256"),
                    "our_pdf_sha256": our_meta.get("pdf_sha256"),
                }
            )
            write_json(cmp_dir / doc.corpus / f"{doc.key}.json", result)
            score = result.get("score", "-")
            if result["status"] != "ok" or n % 25 == 0:
                print(f"[{n}/{len(todo)}] {result['status']} {score} {doc.ident} {result.get('error', '')}", flush=True)
    print(f"compare[{label}]: done", flush=True)
    return 0


def cmd_report(args) -> int:
    import report

    label = resolve_label(args)
    path = report.build(args.store, args.data_root, label, args.baseline, args.corpus)
    print(f"report[{label}]: {path}", flush=True)
    return 0


def cmd_run(args) -> int:
    for step in (cmd_capture, cmd_render, cmd_compare, cmd_report):
        rc = step(args)
        if rc:
            return rc
    return 0


def cmd_corpora(args) -> int:
    """Lists the corpora under the data root with their document counts."""
    for corpus in common.corpora(args.data_root):
        count = sum(1 for _ in common.iter_docs(args.data_root, [corpus.name]))
        print(f"{corpus.name}\t{count}\t{corpus.path}", flush=True)
    return 0


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = parser.add_subparsers(dest="cmd", required=True)
    for name, fn in (("capture", cmd_capture), ("render", cmd_render), ("compare", cmd_compare), ("report", cmd_report), ("run", cmd_run), ("corpora", cmd_corpora)):
        p = sub.add_parser(name)
        p.set_defaults(fn=fn)
        p.add_argument("--store", type=Path, default=common.default_store(), help="output store (default: <main copy>/wps-qa)")
        p.add_argument("--data-root", type=Path, default=common.main_copy_root(), help="where the corpora live (default: the main copy)")
        p.add_argument("--corpus", action="append", help="limit to a corpus (repeatable; `corpora` lists them)")
        p.add_argument("--limit", type=int, default=0, help="first N documents only (smoke runs)")
        p.add_argument("--force", action="store_true", help="redo even when the cached result is fresh")
        p.add_argument("--cli", default=str(default_cli()), help="our CLI binary (default: main copy target/release)")
        p.add_argument("--label", help="name of our render (default: bin-<sha256 of the CLI>)")
        p.add_argument("--jobs", type=int, default=max(1, (os.cpu_count() or 2) // 2))
        p.add_argument("--timeout", type=int, default=240 if name in ("capture", "run") else 300, help="seconds per document")
        p.add_argument("--retry-failed", action="store_true", help="capture: retry documents WPS failed on before")
        p.add_argument("--baseline", help="report: label of an earlier render to diff against")
    args = parser.parse_args(argv)
    args.store = args.store.resolve()
    return args.fn(args)


if __name__ == "__main__":
    sys.exit(main())
