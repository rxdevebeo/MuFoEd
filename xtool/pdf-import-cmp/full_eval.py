"""convert all PDFs with one CLI; text + structure metrics vs the PDF text layer."""
import sys, subprocess, pathlib, json, zipfile, re, collections, difflib, time
sys.stdout.reconfigure(encoding="utf-8")
import pypdfium2 as pdfium
cli, label, out = sys.argv[1], sys.argv[2], pathlib.Path(sys.argv[3])
srcs = [pathlib.Path(p) for p in sys.argv[4:]]
out = out / label; out.mkdir(parents=True, exist_ok=True)
def words(t): return re.findall(r"\w+", t.lower())
def docx_text(x):
    x = re.sub(r"<w:tab/>", " ", x); x = re.sub(r"</w:p>", "\n", x)
    return " ".join(re.findall(r"<w:t[^>]*>([^<]*)</w:t>|\n", x))
def all_xml(z):
    parts = [n for n in z.namelist() if re.match(r"word/(document|header\d*|footer\d*)\.xml$", n)]
    return {n: z.read(n).decode("utf8", "replace") for n in parts}
rows = []
for src in srcs:
    for pdf in sorted(src.glob("*.pdf")):
        key = (src.name + "__" + re.sub(r"[^\w]+", "_", pdf.stem))[:80]
        docx, rep = out / f"{key}.docx", out / f"{key}.json"
        for f in (docx, rep):
            if f.exists(): f.unlink()
        t0 = time.time()
        try:
            r = subprocess.run([cli, "from-pdf", str(pdf), "--pages", "1-5", "--out", str(docx), "--report-out", str(rep)], capture_output=True, text=True, timeout=300)
            rc = r.returncode; err = (r.stderr or "")[-300:]
        except subprocess.TimeoutExpired:
            rc, err = "TIMEOUT", ""
        row = dict(key=key, rc=rc, seconds=round(time.time() - t0, 2))
        if rc not in (0, 1) or not docx.exists():
            row["error"] = err.strip().splitlines()[-1] if err.strip() else str(rc)
            rows.append(row); print(key, "FAIL", row.get("error"), flush=True); continue
        ids = collections.Counter()
        if rep.exists():
            for it in json.load(open(rep, encoding="utf8")).get("issues", []):
                if it.get("stage") == "convert": ids[it["id"]] += it.get("count", 1)
        row["ids"] = dict(ids)
        z = zipfile.ZipFile(docx); xml = all_xml(z); doc = xml.get("word/document.xml", "")
        paras = [p for p in re.findall(r"<w:p[ >].*?</w:p>", re.sub(r"<wp:anchor.*?</wp:anchor>", "", doc, flags=re.S), re.S)]
        ptext = ["".join(re.findall(r"<w:t[^>]*>([^<]*)</w:t>", p)) for p in paras]
        filled = [t for t in ptext if t.strip()]
        row.update(paras=len(paras), avg_chars=round(sum(map(len, filled)) / max(1, len(filled)), 1),
                   anchors=doc.count("<wp:anchor"), lnto=doc.count("<a:lnTo"), grad=doc.count("<a:gradFill"),
                   tables=doc.count("<w:tbl>"), headers=sum(1 for n in xml if "header" in n or "footer" in n),
                   leaders=doc.count("w:leader="), kb=round(sum(len(v) for v in xml.values()) / 1024))
        try:
            d = pdfium.PdfDocument(str(pdf))
            ref = words(" ".join(d[i].get_textpage().get_text_range() for i in range(min(5, len(d)))))
        except Exception:
            ref = []
        got = words(" ".join(docx_text(v) for v in xml.values()))
        row["ref_words"] = len(ref)
        if len(ref) >= 30:
            rc_, gc = collections.Counter(ref), collections.Counter(got)
            row["recall"] = round(sum((rc_ & gc).values()) / len(ref), 4)
            row["extra"] = round(sum((gc - rc_).values()) / len(ref), 4)
            body = words(docx_text(doc))
            row["order"] = round(difflib.SequenceMatcher(None, ref[:3000], body[:3000], autojunk=False).ratio(), 4)
        rows.append(row); print(key, rc, row.get("recall"), row.get("order"), row["seconds"], flush=True)
json.dump(rows, open(out / "rows.json", "w", encoding="utf8"), ensure_ascii=False, indent=1)
