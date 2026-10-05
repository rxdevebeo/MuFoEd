"""Isolate the visual defects without changing the corpus or production code."""
from pathlib import Path
from zipfile import ZipFile
from lxml import etree as E
import hashlib
import json
import subprocess

ROOT = Path(__file__).resolve().parents[3]
OUT = Path(__file__).resolve().parent
SOURCE = ROOT / 'strict-ooxml-core/tests/docx/2. Мероприятия по Лайму.docx'
CLI = ROOT / 'target/debug/strict-ooxml.exe'
NS = {'w': 'http://schemas.openxmlformats.org/wordprocessingml/2006/main'}
W = '{' + NS['w'] + '}'
S = {'s': 'http://www.w3.org/2000/svg'}

def variant(name, edit):
    dest = OUT / (name + '.docx')
    with ZipFile(SOURCE) as src, ZipFile(dest, 'w') as dst:
        for item in src.infolist():
            data = src.read(item.filename)
            if item.filename == 'word/document.xml':
                doc = E.fromstring(data)
                edit(doc)
                data = E.tostring(doc, encoding='UTF-8', xml_declaration=True, standalone=True)
            dst.writestr(item, data)
    return dest

def remove_paragraph_toggles(doc):
    p = doc.find('w:body', NS)[36]
    for node in p.xpath('./w:pPr/w:rPr/w:i | ./w:pPr/w:rPr/w:b', namespaces=NS):
        node.getparent().remove(node)

def remove_zero_table_width(doc):
    table = doc.find('w:body', NS)[39]
    node = table.find('w:tblPr/w:tblW', NS)
    node.getparent().remove(node)

def use_bundled_font(doc):
    p = doc.find('w:body', NS)[36]
    for fonts in p.xpath('.//w:rFonts', namespaces=NS):
        for key in ['ascii', 'hAnsi', 'cs']:
            fonts.set(W + key, 'Times New Roman')

def analyze_svg(folder):
    result = {'pages': len(list(folder.glob('page-*.svg'))), 'heading': [], 'table_prefix': []}
    for file in sorted(folder.glob('page-*.svg')):
        tree = E.parse(str(file))
        texts = tree.xpath('//s:text', namespaces=S)
        for i, text in enumerate(texts):
            if text.text == 'Приложение ' and text.get('fill') == '#0033cc':
                result['heading'] = [{'text': t.text, **dict(t.attrib)} for t in texts[i:i+2]]
            # A prefix sufficient to show one-character wrapping; avoid copying email data.
            if text.text == 'В' and i+1 < len(texts) and texts[i+1].text == 'и':
                result['table_prefix'] = [{'text': t.text, 'x': t.get('x'), 'y': t.get('y')} for t in texts[i:i+8]]
            elif text.text and text.text.startswith('Вилониса'):
                result['table_prefix'] = [{'text': text.text.split()[0], 'x': text.get('x'), 'y': text.get('y')}]
    return result

cases = {'original': SOURCE,
         'heading-no-paragraph-toggles': variant('heading-no-paragraph-toggles', remove_paragraph_toggles),
         'table-no-zero-width': variant('table-no-zero-width', remove_zero_table_width),
         'heading-bundled-font': variant('heading-bundled-font', use_bundled_font)}
results = {'source_sha256': hashlib.sha256(SOURCE.read_bytes()).hexdigest(), 'cases': {}}
for name, file in cases.items():
    folder = OUT / name
    proc = subprocess.run([str(CLI), 'render', str(file), '--transitional', '--out', str(folder)], capture_output=True)
    (OUT / (name + '.log')).write_bytes(proc.stdout + proc.stderr)
    results['cases'][name] = {'exit': proc.returncode, **analyze_svg(folder)}

try:
    from fontTools.ttLib import TTFont
    font = TTFont('C:/Windows/Fonts/segoeui.ttf')
    cmap = font.getBestCmap()
    units = font['head'].unitsPerEm
    def width(text):
        return sum(font['hmtx'][cmap[ord(c)]][0] for c in text) / units * 16
    results['segoe_ui_16px_hmtx'] = {
        'heading_word_and_space': width('Приложение '),
        'heading_word_only': width('Приложение'),
        'layout_advance': 200.6 - 113.4,
        'font_sha256': hashlib.sha256(Path('C:/Windows/Fonts/segoeui.ttf').read_bytes()).hexdigest(),
        'note': 'Unshaped advance sum; actual browser shaping can differ slightly.'}
except ImportError:
    results['segoe_ui_16px_hmtx'] = 'fontTools unavailable'
(OUT / 'evidence.json').write_text(json.dumps(results, ensure_ascii=False, indent=2), encoding='utf-8')
print(json.dumps(results, ensure_ascii=False, indent=2))
