"""Diagnostic: absolute PDF glyph baselines, no inferred font-height factor."""
import hashlib,json,re
from pathlib import Path
import pdfplumber
from lxml import etree
root=Path('D:/projects/StrictLib')
dest=root/'target/remediation-2026-10-06'
keys={54:['HVR-I','L15996','H16142','L16055','L16117','H16233'],56:['-M','H','8994','6371','11719','14766','7028'],104:['A,C','B,D','modern']}
def occurrences(text,key):
    return [m.start() for m in re.finditer(re.escape(key),text)]
report={'reference':'WPS, not Word','coordinates':'absolute glyph origin, top-left, 96 dpi','tolerance_px':0.25,'pages':{}}
for number,needles in keys.items():
    pdfpath=root/f'docs/audit-2026-10-04/visual-thesis/wps-reference/page-{number}.pdf'
    svgpath=dest/f'clio-svg/page-{number}.svg'
    with pdfplumber.open(pdfpath) as pdf:
        page=pdf.pages[0]
        ptext=''; ppoints=[]
        for char in page.chars:
            for ch in char['text']:
                if not ch.isspace():
                    ptext+=ch; ppoints.append((char['matrix'][4]*4/3,(page.height-char['matrix'][5])*4/3))
    stext=''; spoints=[]
    for node in etree.parse(str(svgpath)).xpath('//*[local-name()="text"]'):
        text=node.text or ''
        xs=[float(x) for x in (node.get('x') or '0').split()]
        y=float(node.get('y') or '0')
        for i,ch in enumerate(text):
            if not ch.isspace():
                stext+=ch
                spoints.append((xs[i] if len(xs)==len(text) else xs[0] if i==0 else None,y))
    rec={'pdf_sha256':hashlib.sha256(pdfpath.read_bytes()).hexdigest(),'svg_sha256':hashlib.sha256(svgpath.read_bytes()).hexdigest(),'points':[]}
    for key in needles:
        pi=occurrences(ptext,key); si=occurrences(stext,key)
        row={'key':key,'reference_occurrences':len(pi),'actual_occurrences':len(si)}
        if len(pi)==1 and len(si)==1:
            p=ppoints[pi[0]]; s=spoints[si[0]]
            row.update(reference_px=p,actual_px=s)
            if s[0] is not None:
                dx=s[0]-p[0]; dy=s[1]-p[1]
                row.update(dx=dx,dy=dy,status='PASS' if max(abs(dx),abs(dy))<=.25 else 'FAIL')
            else: row['status']='UNMEASURABLE'
        else: row['status']='AMBIGUOUS_OR_MISSING'
        rec['points'].append(row)
    report['pages'][str(number)]=rec
(dest/'wps-absolute-baselines.json').write_text(json.dumps(report,indent=2),encoding='utf-8')
for page,rec in report['pages'].items():
    print(page,[(r['key'],r['status'],round(r.get('dx',0),3),round(r.get('dy',0),3)) for r in rec['points']])
