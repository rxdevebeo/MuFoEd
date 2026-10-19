"""WPS-export every docx of base/head and measure ink F1 / SSIM against the source PDF pages."""
import sys, re, json, subprocess, pathlib, psutil
sys.stdout.reconfigure(encoding="utf-8")
import numpy as np, pypdfium2 as pdfium
from scipy.ndimage import binary_dilation
from skimage.metrics import structural_similarity as ssim
worker, full = sys.argv[1], pathlib.Path(sys.argv[2]); LABELS = sys.argv[3].split(","); OUT = sys.argv[4]; srcs = [pathlib.Path(p) for p in sys.argv[5:]]
src_of = {}
for src in srcs:
    for pdf in sorted(src.glob("*.pdf")):
        src_of[(src.name + "__" + re.sub(r"[^\w]+", "_", pdf.stem))[:80]] = pdf
def export(docx):
    out = docx.with_suffix(".wps.pdf"); pid = docx.with_suffix(".pids")
    if out.exists(): return out
    try:
        subprocess.run([sys.executable, "-I", worker, str(docx), str(out), str(pid)], capture_output=True, text=True, timeout=180)
    except subprocess.TimeoutExpired:
        for p in json.loads(pid.read_text() or "[]") if pid.exists() else []:
            try: psutil.Process(p).kill()
            except Exception: pass
    return out if out.exists() else None
def pages(p, n):
    d = pdfium.PdfDocument(str(p)); return [d[i].render(scale=40/72, grayscale=True).to_pil().convert("L") for i in range(min(n, len(d)))]
res = {}
for key, pdf in src_of.items():
    try: sp = pages(pdf, 5)
    except Exception: continue
    row = {}
    for label in LABELS:
        docx = full / label / f"{key}.docx"
        if not docx.exists(): continue
        out = export(docx)
        if not out: row[label] = None; continue
        try: rp = pages(out, len(sp))
        except Exception: row[label] = None; continue
        f1s, ss = [], []
        for i, s in enumerate(sp):
            if i >= len(rp): f1s.append(0.0); ss.append(0.0); continue
            a = np.array(s) < 200; r = rp[i].resize(s.size); b = np.array(r) < 200
            den = a.sum() + b.sum()
            f1s.append(float(((a & binary_dilation(b, iterations=2)).sum() + (b & binary_dilation(a, iterations=2)).sum()) / den) if den else 1.0)
            ss.append(float(ssim(np.array(s), np.array(r))))
        row[label] = dict(ink=round(float(np.mean(f1s)), 4), ssim=round(float(np.mean(ss)), 4), pages=len(pdfium.PdfDocument(str(out))), src_pages=len(sp))
    res[key] = row
    print(key, row, flush=True)
json.dump(res, open(full / OUT, "w", encoding="utf8"), ensure_ascii=False, indent=1)
