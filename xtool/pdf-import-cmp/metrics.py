import sys, re, json, zipfile, difflib, collections, pathlib
import numpy as np, pypdfium2 as pdfium
from PIL import Image, ImageFilter
from skimage.metrics import structural_similarity as ssim
cmp = pathlib.Path(sys.argv[1]); img = cmp/"img"; img.mkdir(exist_ok=True)
NP = 6; DPI = 60
def pdf_pages(p, n=None):
    d = pdfium.PdfDocument(str(p)); k = len(d) if n is None else min(n, len(d))
    texts, ims, sizes = [], [], []
    for i in range(k):
        pg = d[i]; texts.append(pg.get_textpage().get_text_range())
        sizes.append(pg.get_size())
        ims.append(pg.render(scale=DPI/72, grayscale=True).to_pil().convert("L"))
    return len(d), texts, ims, sizes
def words(t): return re.findall(r"\w+", t.lower())
def ink(im, size):
    a = np.array(im.resize(size)) < 200
    return a
def docx_stats(p):
    z = zipfile.ZipFile(p); x = z.read("word/document.xml").decode("utf8")
    paras = re.findall(r"<w:p[ >].*?</w:p>", x, re.S)
    ptext = [ "".join(re.findall(r"<w:t[^>]*>([^<]*)</w:t>", q)) for q in paras]
    c = lambda pat: len(re.findall(pat, x))
    return dict(paras=len(paras), empty=sum(1 for t in ptext if not t.strip()),
        avg_chars=round(np.mean([len(t) for t in ptext if t.strip()] or [0]),1),
        tbl=c(r"<w:tbl>"), sect=c(r"<w:sectPr"), cols_multi=c(r'<w:cols [^>]*w:num="[2-9]'),
        numPr=c(r"<w:numPr>"), anchor=c(r"<wp:anchor"), inline=c(r"<wp:inline"), txbx=c(r"<w:txbxContent"),
        framePr=c(r"<w:framePr"), pgbr=c(r'w:type="page"'), tabs=c(r"<w:tab/>"), exact=c(r'w:lineRule="exact"'),
        ind=c(r"<w:ind "), styles_h=c(r'<w:pStyle w:val="[Hh]eading'))
rows = []
for src in sorted((cmp/"src").glob("*.pdf")):
    n = src.stem; total, st, sims, ssz = pdf_pages(src, NP)
    sw = words(" ".join(st)); scount = collections.Counter(sw)
    for kind in ("ours", "p2d"):
        r = dict(doc=n, conv=kind, src_pages=len(st))
        rp = cmp/kind/f"{n}.wps.pdf"
        rtotal, rt, rims, rsz = pdf_pages(rp)
        r["out_pages"] = rtotal
        rw = words(" ".join(rt)); rcount = collections.Counter(rw)
        r["word_recall"] = round(sum((scount & rcount).values())/max(1,len(sw)),3)
        r["word_extra"] = round(sum((rcount - scount).values())/max(1,len(sw)),3)
        sm = difflib.SequenceMatcher(None, sw[:4000], rw[:4000], autojunk=False)
        r["order"] = round(sm.ratio(),3)
        ious, ss = [], []
        for i in range(len(sims)):
            if i >= len(rims): ious.append(0); ss.append(0); continue
            size = sims[i].size
            a = ink(sims[i], size); b = ink(rims[i], size)
            from scipy.ndimage import binary_dilation
            ad, bd = binary_dilation(a, iterations=2), binary_dilation(b, iterations=2)
            inter = (a & bd).sum() + (b & ad).sum(); den = a.sum() + b.sum()
            ious.append(inter/den if den else 1)
            ss.append(ssim(np.array(sims[i]), np.array(rims[i].resize(size))))
            if i < 3:
                w,h = size; canvas = Image.new("L",(w*2+10,h),255); canvas.paste(sims[i],(0,0)); canvas.paste(rims[i].resize(size),(w+10,0))
                canvas.save(img/f"{n}_{kind}_p{i+1}.png")
        r["ink_f1"] = round(float(np.mean(ious)),3); r["ssim"] = round(float(np.mean(ss)),3)
        r["page_size_ok"] = all(abs(a[0]-b[0])<3 and abs(a[1]-b[1])<3 for a,b in zip(ssz, rsz))
        r.update(docx_stats(cmp/kind/f"{n}.docx"))
        rows.append(r)
json.dump(rows, open(cmp/"metrics.json","w"), indent=1)
keys = ["doc","conv","src_pages","out_pages","page_size_ok","word_recall","word_extra","order","ink_f1","ssim"]
print("\t".join(keys))
for r in rows: print("\t".join(str(r[k]) for k in keys))
keys2 = ["doc","conv","paras","empty","avg_chars","tbl","sect","cols_multi","numPr","anchor","inline","txbx","framePr","pgbr","tabs","exact","ind","styles_h"]
print(); print("\t".join(keys2))
for r in rows: print("\t".join(str(r[k]) for k in keys2))
