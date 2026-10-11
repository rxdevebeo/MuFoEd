"""Exports ONE document to PDF through WPS Writer's COM server (KWPS.Application).

Run by `wps_qa.py capture` in a child process, one document per process, so a
WPS that hangs on a modal dialog or a pathological file costs one timeout and
never the whole run. The parent owns the timeout; this script only reports.

    python -I wps_worker.py <src.docx> <out.pdf> <pidfile>

The pids of the wps.exe processes this worker started are written to <pidfile>
right after the COM server comes up, so the parent can kill exactly them (and
never a WPS window the user has open) when the export does not finish.

Prints one JSON object on stdout.
"""

from __future__ import annotations

import json
import os
import sys
import time

import psutil
import pythoncom
import win32com.client

WD_EXPORT_PDF = 17
WD_DO_NOT_SAVE = 0
# A password no real document uses: a protected file then fails to open
# instead of raising a password dialog that would hang the export.
BOGUS_PASSWORD = "__wpsqa_no_password__"


def wps_pids() -> set[int]:
    pids = set()
    for proc in psutil.process_iter(["name"]):
        if (proc.info.get("name") or "").lower() == "wps.exe":
            pids.add(proc.pid)
    return pids


def main() -> int:
    src, out, pidfile = (os.path.abspath(a) for a in sys.argv[1:4])
    result: dict = {"ok": False}
    started = time.time()
    before = wps_pids()
    pythoncom.CoInitialize()
    app = doc = None
    try:
        app = win32com.client.DispatchEx("KWPS.Application")
        ours = sorted(wps_pids() - before)
        with open(pidfile, "w", encoding="utf-8") as f:
            json.dump(ours, f)
        result["wps_pids"] = ours
        result["wps_version"] = str(getattr(app, "Build", "") or app.Version)
        app.Visible = False
        app.DisplayAlerts = 0
        t = time.time()
        # Open(FileName, ConfirmConversions, ReadOnly, AddToRecentFiles,
        #      PasswordDocument, PasswordTemplate, Revert,
        #      WritePasswordDocument, WritePasswordTemplate)
        doc = app.Documents.Open(
            src, False, True, False, BOGUS_PASSWORD, BOGUS_PASSWORD, False, BOGUS_PASSWORD, BOGUS_PASSWORD
        )
        result["open_s"] = round(time.time() - t, 2)
        # Print layout, final view without markup: the document as it would be
        # printed after accepting changes, which is what our renderer draws.
        for setter in (
            lambda: setattr(doc.ActiveWindow.View, "Type", 3),
            lambda: setattr(doc.ActiveWindow.View, "RevisionsView", 0),
            lambda: setattr(doc.ActiveWindow.View, "ShowRevisionsAndComments", False),
        ):
            try:
                setter()
            except Exception:  # noqa: BLE001 - optional view settings
                pass
        t = time.time()
        if os.path.exists(out):
            os.remove(out)
        # ExportAsFixedFormat(OutputFileName, ExportFormat, OpenAfterExport,
        #                     OptimizeFor=print)
        doc.ExportAsFixedFormat(out, WD_EXPORT_PDF, False, 0)
        result["export_s"] = round(time.time() - t, 2)
        result["ok"] = os.path.isfile(out) and os.path.getsize(out) > 0
        if not result["ok"]:
            result["error"] = "export produced no file"
    except Exception as exc:  # noqa: BLE001 - every failure is a reported result
        result["error"] = f"{type(exc).__name__}: {exc}"
    finally:
        try:
            if doc is not None:
                doc.Close(WD_DO_NOT_SAVE)
        except Exception:  # noqa: BLE001
            pass
        try:
            if app is not None:
                app.Quit()
        except Exception:  # noqa: BLE001
            pass
        result["seconds"] = round(time.time() - started, 2)
        sys.stdout.write(json.dumps(result, ensure_ascii=True) + "\n")
        sys.stdout.flush()
    return 0


if __name__ == "__main__":
    sys.exit(main())
