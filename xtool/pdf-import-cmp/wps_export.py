import sys, json, subprocess, pathlib, psutil
worker, cmp = sys.argv[1], pathlib.Path(sys.argv[2])
for kind in ("ours", "p2d"):
    for d in sorted((cmp/kind).glob("*.docx")):
        out = d.with_suffix(".wps.pdf"); pid = d.with_suffix(".pids")
        try:
            r = subprocess.run([sys.executable, "-I", worker, str(d), str(out), str(pid)], capture_output=True, text=True, timeout=180)
            print(kind, d.stem, r.stdout.strip()[:200], flush=True)
        except subprocess.TimeoutExpired:
            for p in json.loads(pid.read_text() or "[]"):
                try: psutil.Process(p).kill()
                except Exception: pass
            print(kind, d.stem, "TIMEOUT", flush=True)
