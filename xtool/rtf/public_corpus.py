#!/usr/bin/env python3
"""Measure isolated public CLI imports against frozen independent WPS body facts.

This is a measurement, not a reviewed expectations generator. Structure and
visual verdicts are deliberately separate from exact body-text comparison.
"""
import argparse
import ctypes
import hashlib
import json
import os
from pathlib import Path
import platform
import subprocess
import time
import xml.etree.ElementTree as ET
import zipfile

import psutil
from census import inventory, write_json
from docx_facts import local, WML


def visible(node):
    name = local(node.tag)
    if node.tag not in {"{" + ns + "}" + name for ns in WML}:
        return "".join(visible(child) for child in node)
    if name in {"pPr", "rPr", "sdtPr", "del", "moveFrom", "txbxContent", "instrText", "delText"}:
        return ""
    if name == "t":
        return node.text or ""
    if name == "sym":
        code = int(next(v for k, v in node.attrib.items() if local(k) == "char"), 16)
        return chr(code if code >= 0xF000 else 0xF000 + code)
    if name == "tab":
        return "\t"
    if name == "cr":
        return "\n"
    if name == "br":
        kind = next((v for k, v in node.attrib.items() if local(k) == "type"), "textWrapping")
        return {"page": "\f", "column": "\v"}.get(kind, "\n")
    if name in {"softHyphen", "noBreakHyphen"}:
        return {"softHyphen": "\u00ad", "noBreakHyphen": "\u2011"}[name]
    return "".join(visible(child) for child in node)


def paragraphs(node):
    if local(node.tag) in {"del", "moveFrom", "txbxContent"}:
        return []
    if local(node.tag) == "p":
        return [visible(node)]
    return [text for child in node for text in paragraphs(child)]


def body_text(path):
    with zipfile.ZipFile(path) as package:
        info = package.getinfo("word/document.xml")
        if info.file_size > 128 * 1024 * 1024:
            raise ValueError("XML part budget exceeded")
        return paragraphs(ET.fromstring(package.read(info)))


def final_peak_rss(process):
    """Windows kernel retains process counters through the owned handle."""
    if os.name != "nt":
        return None
    from ctypes import wintypes
    class Counters(ctypes.Structure):
        _fields_ = [("cb", wintypes.DWORD), ("faults", wintypes.DWORD)] + [(name, ctypes.c_size_t) for name in (
            "peak", "working", "paged_peak", "paged", "nonpaged_peak", "nonpaged", "pagefile", "pagefile_peak")]
    counters = Counters()
    counters.cb = ctypes.sizeof(counters)
    fn = ctypes.windll.psapi.GetProcessMemoryInfo
    fn.argtypes = [wintypes.HANDLE, ctypes.POINTER(Counters), wintypes.DWORD]
    fn.restype = wintypes.BOOL
    if not fn(wintypes.HANDLE(int(process._handle)), ctypes.byref(counters), counters.cb):
        return None
    return counters.peak


def measured(command, logfile, timeout, rss_limit):
    start = time.monotonic()
    sampled = 0
    stop = None
    with logfile.open("wb") as log:
        process = subprocess.Popen(command, stdout=log, stderr=subprocess.STDOUT,
                                   creationflags=getattr(subprocess, "CREATE_NO_WINDOW", 0))
        monitored = psutil.Process(process.pid)
        while process.poll() is None:
            try:
                info = monitored.memory_info()
                sampled = max(sampled, getattr(info, "peak_wset", info.rss))
            except psutil.NoSuchProcess:
                pass
            if sampled > rss_limit:
                stop = "memory limit"
            elif time.monotonic() - start > timeout:
                stop = "timeout"
            if stop:
                process.kill()
                break
            time.sleep(0.01)
        code = process.wait()
        final_peak = final_peak_rss(process)
    elapsed = time.monotonic() - start
    peak = max(sampled, final_peak or 0)
    verdict = "Fail" if stop or peak > rss_limit else "Pass" if final_peak is not None else "Blocked"
    return {"exit_code": code, "elapsed_seconds": round(elapsed, 4), "peak_rss_bytes": peak,
            "peak_method": "Windows final process counters" if final_peak is not None else "sampled only",
            "resource_verdict": verdict, "stop_reason": stop}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--cli", type=Path, required=True)
    parser.add_argument("--corpus", type=Path, default=Path("testdata/CC0_RTF"))
    parser.add_argument("--references", type=Path, default=Path("target/rtf-import/R01/wps-facts-v2"))
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--timeout", type=int, default=30)
    parser.add_argument("--rss-mib", type=int, default=512)
    args = parser.parse_args()
    args.output.mkdir(parents=True, exist_ok=False)
    corpus = inventory(args.corpus)
    rows = []
    receipt = {"claim": "public_path_development_measurement_not_full_acceptance", "manifest_sha256": corpus["manifest_sha256"],
               "cli_sha256": hashlib.sha256(args.cli.read_bytes()).hexdigest(), "platform": platform.platform(),
               "psutil": psutil.__version__, "timeout_seconds": args.timeout, "rss_limit_bytes": args.rss_mib * 1024**2,
               "options": ["--wps-text", "--allow-unsigned-unicode", "--drop-nul"], "files": rows}
    for index, source in enumerate(corpus["files"], 1):
        row = {"filename": source["filename"], "sha256": source["expected_sha256"], "integrity": source["integrity"]}
        try:
            if source["integrity"] != "Pass":
                raise ValueError("source integrity failed")
            out = args.output / f"{index:03}.docx"
            command = [str(args.cli.resolve()), "import-rtf", str((args.corpus / source["filename"]).resolve()),
                       "--out", str(out.resolve()), *receipt["options"]]
            row.update(measured(command, args.output / f"{index:03}.log", args.timeout, receipt["rss_limit_bytes"]))
            if row["exit_code"] not in {0, 1}:
                raise ValueError("public import failed")
            report_path = Path(str(out) + ".rtf-report.json")
            report = json.loads(report_path.read_text(encoding="utf8"))
            row.update(outcome=report["status"], import_loss_count=report["import"]["loss_count"], writer_loss_count=report["writer"]["loss_count"],
                       output_bytes=out.stat().st_size, output_sha256=hashlib.sha256(out.read_bytes()).hexdigest(),
                       report_sha256=hashlib.sha256(report_path.read_bytes()).hexdigest())
            facts_path = args.references / "files" / f"{index:03}" / "facts.json"
            facts = json.loads(facts_path.read_text(encoding="utf8"))
            expected = next(r for r in facts["regions"] if r["kind"] == "document")
            actual = body_text(out)
            expected_text = [p["text"] for p in expected["paragraphs"]]
            row.update(text_verdict="Pass" if actual == expected_text else "Fail", actual_paragraphs=len(actual), expected_paragraphs=len(expected_text),
                       oracle_facts_sha256=hashlib.sha256(facts_path.read_bytes()).hexdigest(),
                       first_mismatch=next((i for i, (a, b) in enumerate(zip(actual, expected_text)) if a != b), None))
            row["status"] = "Measured"
        except (OSError, ValueError, KeyError, StopIteration, ET.ParseError, zipfile.BadZipFile) as error:
            row.update(status="Fail", reason=str(error))
        rows.append(row)
        write_json(args.output / "results.json", receipt)
        print(f"{index:03}/100 {row['status']} {row.get('text_verdict')} {row.get('resource_verdict')}", flush=True)
    return 0 if len(rows) == 100 and all(r["status"] == "Measured" and r["text_verdict"] == r["resource_verdict"] == "Pass" for r in rows) else 1


if __name__ == "__main__":
    raise SystemExit(main())
