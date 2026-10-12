"""Compare saved SVG text ledgers with independent WPS PDF exports.

Exact text-line matches on the first two pages are coordinate probes, not a
fidelity score. PDF and SVG have different baselines; report x only.
"""
import collections, glob, html, json, re, zipfile
from pathlib import Path
import pdfplumber
from lxml import etree

ROOT = Path('render_out/math-layout-audit')
NS = {'w': 'http://schemas.openxmlformats.org/wordprocessingml/2006/main'}
KNOWN = {'calibri','carlito','cambria','caladea','arial','helvetica',
         'liberation sans','arimo','times new roman','liberation serif','tinos',
         'courier new','liberation mono','cousine','cambria math','stix two math',
         'stix2 math','stixgeneral'}
def norm(s):
    return re.sub(r'\s+', '', html.unescape(s)).casefold()
def attrs(s):
    return dict(re.findall(r'([\w:-]+)="([^"]*)"', s))

rows = []
for selected in json.loads((ROOT/'selection.json').read_text()):
    name = selected['name']
    prefix = name[:3]
    reference = ROOT/'reference'/name.replace('.docx', '.pdf')
    ours = ROOT/'ours'/prefix
    metadata = json.loads((ours/'metadata.json').read_text())
    ledgers = []
    with (ours/'text-pages.jsonl').open(encoding='utf8') as f:
        for _ in range(2):
            line = f.readline()
            if line: ledgers.append(json.loads(line))
    probes, fonts = [], collections.Counter()
    with pdfplumber.open(reference) as pdf:
        reference_pages = len(pdf.pages)
        for page, ledger in zip(pdf.pages[:2], ledgers):
            fonts.update(c['fontname'].split('+')[-1] for c in page.chars)
            svg_lines = collections.defaultdict(list)
            for segment in ledger['segments']:
                a = attrs(segment['attributes'])
                svg_lines[round(float(a['y']), 1)].append(
                    (float(a['x'].split()[0]), html.unescape(segment['text'])))
            svg_by_text = {}
            for y, items in svg_lines.items():
                items.sort(key=lambda item:item[0])
                text = ''.join(item[1] for item in items)
                if len(norm(text)) >= 20:
                    svg_by_text.setdefault(norm(text), []).append((items[0][0], y, text))
            pdf_lines = collections.defaultdict(list)
            for char in page.chars:
                pdf_lines[round(char['top'], 1)].append(char)
            for chars in pdf_lines.values():
                chars.sort(key=lambda c:c['x0'])
                text = ''.join(c['text'] for c in chars)
                matches = svg_by_text.get(norm(text), [])
                if len(matches) == 1:
                    x, y, _ = matches[0]
                    probes.append({'page':ledger['page'], 'text':text[:100],
                        'svg_x':x,'reference_x':round(chars[0]['x0']*4/3,3),
                        'delta_x':round(x-chars[0]['x0']*4/3,3)})
    rows.append({'name':name,'ours_pages':len(metadata['pages']),
        'wps_pages':reference_pages,'fonts_first_two_pdf_pages':dict(fonts),
        'exact_line_probes_first_two_pages':probes})

declarations = collections.defaultdict(set)
for filename in glob.glob('testdata/CC0_DOCX_MATH/*.docx'):
    with zipfile.ZipFile(filename) as archive:
        for part in ['word/document.xml','word/styles.xml']:
            if part not in archive.namelist(): continue
            root = etree.fromstring(archive.read(part))
            for node in root.findall('.//w:rFonts',NS):
                for key in ['ascii','hAnsi','eastAsia','cs']:
                    family = node.get('{'+NS['w']+'}'+key)
                    if family: declarations[family].add(Path(filename).name)
output = {'method':'WPS local export; first two pages exact line x probes; font declarations may be unused',
          'selected':rows,
          'declared_unknown_families':{k:sorted(v) for k,v in sorted(declarations.items()) if k.strip().lower() not in KNOWN},
          'declared_known_families':{k:len(v) for k,v in sorted(declarations.items()) if k.strip().lower() in KNOWN}}
(ROOT/'reference-comparison.json').write_text(json.dumps(output,ensure_ascii=False,indent=2),encoding='utf8')
print(json.dumps({'documents':len(rows),'exact_line_probes':sum(len(x['exact_line_probes_first_two_pages']) for x in rows),
                  'unknown_declared_families':len(output['declared_unknown_families'])}))
