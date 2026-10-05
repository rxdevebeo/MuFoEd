"""Evidence and isolated DOCX mutations; original and production code stay intact."""
from pathlib import Path
from zipfile import ZipFile
from lxml import etree as E
from collections import Counter
import hashlib, json, subprocess
ROOT = Path(__file__).resolve().parents[3]
OUT = Path(__file__).resolve().parent
SOURCE = ROOT / 'strict-ooxml-core/tests/docx/1. First-Steps-in-Programming.docx'
NS = {'w': 'http://schemas.openxmlformats.org/wordprocessingml/2006/main',
      'wp': 'http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing',
      'wp14': 'http://schemas.microsoft.com/office/word/2010/wordprocessingDrawing'}
W = '{' + NS['w'] + '}'
S = {'s': 'http://www.w3.org/2000/svg'}

def copy(name, edit):
    file = OUT / (name + '.docx')
    with ZipFile(SOURCE) as src, ZipFile(file, 'w') as dst:
        for entry in src.infolist():
            data = src.read(entry.filename)
            if entry.filename == 'word/document.xml':
                tree = E.fromstring(data); edit(tree)
                data = E.tostring(tree, encoding='UTF-8', xml_declaration=True, standalone=True)
            dst.writestr(entry, data)
    return file

def no_headers(tree):
    for node in tree.xpath('//w:headerReference', namespaces=NS):
        node.getparent().remove(node)

def explicit_relative_widths(tree):
    # Isolate horizontal percentage sizing only, not group transforms or vertical positions.
    page_w = int(tree.xpath('//w:pgSz/@w:w', namespaces=NS)[-1]) * 635
    for anchor in tree.xpath('//wp:anchor', namespaces=NS):
        pct = anchor.find('wp14:sizeRelH/wp14:pctWidth', NS)
        rel = anchor.find('wp14:sizeRelH', NS)
        ext = anchor.find('wp:extent', NS)
        if pct is not None and rel.get('relativeFrom') == 'page' and ext is not None:
            ext.set('cx', str(round(page_w * int(pct.text) / 100000)))

def render(name, file):
    dest = OUT / name
    proc = subprocess.run([str(ROOT/'target/debug/strict-ooxml.exe'), 'render', str(file),
                           '--transitional', '--out', str(dest)], capture_output=True)
    (OUT/(name+'.log')).write_bytes(proc.stdout+proc.stderr)
    return {'exit':proc.returncode, 'pages':len(list(dest.glob('page-*.svg')))}

def texts(folder, page):
    return E.parse(str(OUT/folder/f'page-{page}.svg')).xpath('//s:text', namespaces=S)

results = {'source_sha256':hashlib.sha256(SOURCE.read_bytes()).hexdigest()}
with ZipFile(SOURCE) as z:
    tree = E.fromstring(z.read('word/document.xml'))
    shadings = tree.xpath('//w:p/w:pPr/w:shd[@w:fill="F7F7F7"]', namespaces=NS)
    results['grey_paragraphs_input'] = len(shadings)
    results['code_source'] = []
    for p in tree.xpath('//w:p', namespaces=NS):
        t = ''.join(p.xpath('./w:r/w:t/text()', namespaces=NS))
        if t == 'Console.WriteLine("Welcome to coding");':
            results['code_source'].append({'text':t, 'tabs':len(p.xpath('.//w:tab[not(parent::w:tabs)]', namespaces=NS)),
                                         'runs':p.xpath('./w:r/w:t/text()', namespaces=NS)})
    results['margins'] = [dict(n.attrib) for n in tree.xpath('//w:pgMar', namespaces=NS)]

results['renders'] = {'original':render('original',SOURCE),
                      'no-headers':render('no-headers',copy('no-headers',no_headers)),
                      'explicit-relative-widths':render('explicit-relative-widths',copy('explicit-relative-widths',explicit_relative_widths))}
results['grey_svg_shapes'] = sum(len(E.parse(str(f)).xpath('//*[@fill="#f7f7f7"]'))
                               for f in (OUT/'original').glob('page-*.svg'))
for variant in ['original','explicit-relative-widths']:
    ts = texts(variant,1)
    results[variant+'_title_positions'] = [{'text':t.text,'x':t.get('x'),'y':t.get('y')}
                                          for t in ts if t.text in ['FIRST ','STEPS ','IN ','PROGRAMMING']]
ts=texts('original',2)
for i,t in enumerate(ts):
    if t.text == 'Console.WriteLine(':
        results['code_svg']=[{'text':a.text,'x':a.get('x'),'font':a.get('font-family'),'size':a.get('font-size')} for a in ts[i:i+5]]
        break
def signature(t):
    return (t.text,t.get('x'),t.get('y'),t.get('font-family'),t.get('font-size'))
bands=[]
for page in range(2,results['renders']['original']['pages']+1):
    full=texts('original',page); body=texts('no-headers',page)
    counts=Counter(signature(t) for t in body); headers=[]
    for t in full:
        key=signature(t)
        if counts[key]:counts[key]-=1
        else:headers.append(t)
    # Baseline-band proxy only, not an independent ink intersection oracle.
    nearby=[(h,b) for h in headers for b in body
            if abs(float(h.get('y'))-float(b.get('y'))) < max(float(h.get('font-size')),float(b.get('font-size')))*0.8]
    if nearby:
        h,b=nearby[0]
        bands.append({'page':page,'header_baseline':h.get('y'),'body_baseline':b.get('y'),
                      'header_size':h.get('font-size'),'body_size':b.get('font-size')})
results['header_body_near_baseline_pages']=bands
try:
    from fontTools.ttLib import TTFont
    f=TTFont('C:/Windows/Fonts/times.ttf');cmap=f.getBestCmap();upem=f['head'].unitsPerEm
    results['fallback_times_13_333_advance']={t:sum(f['hmtx'][cmap[ord(c)]][0] for c in t)/upem*13.333
                                            for t in ['Console.WriteLine(','"Welcome ',' ' ,');']}
except ImportError:pass
results['source_unchanged']=results['source_sha256']==hashlib.sha256(SOURCE.read_bytes()).hexdigest()
(OUT/'evidence.json').write_text(json.dumps(results,ensure_ascii=False,indent=2),encoding='utf-8')
print(json.dumps({k:v for k,v in results.items() if k not in ['margins','header_body_near_baseline_pages']},ensure_ascii=False,indent=2))
print('near-baseline pages',len(bands),'first',bands[:4])
