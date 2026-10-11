"""Print only relevant OOXML properties, without namespace boilerplate."""
import glob, json, zipfile
from lxml import etree as E
W = 'http://schemas.openxmlformats.org/wordprocessingml/2006/main'
ns = {'w': W}
def properties(node):
    if node is None: return None
    return [{E.QName(x).localname:{E.QName(k).localname:v for k,v in x.attrib.items()}} for x in node]
for prefix in ['019','007','081']:
    name = glob.glob('testdata/CC0_DOCX_MATH/'+prefix+'*')[0]
    root = E.fromstring(zipfile.ZipFile(name).read('word/document.xml'))
    if prefix == '019':
        records = [{'text':''.join(p.xpath('.//w:t/text()',namespaces=ns))[:100], 'props':properties(p.find('w:pPr',ns))} for p in root.findall('w:body/w:p',ns)[:18]]
    else:
        records = []
        for cell in root.findall('.//w:tc',ns):
            text = ''.join(cell.xpath('.//w:t/text()',namespaces=ns))
            if 'Compounds' in text or 'Source' in text:
                table = cell.getparent().getparent()
                records.append({'text':text[:100], 'row':properties(cell.getparent().find('w:trPr',ns)), 'cell':properties(cell.find('w:tcPr',ns)), 'table':properties(table.find('w:tblPr',ns)), 'grid':[dict(x.attrib) for x in table.findall('w:tblGrid/w:gridCol',ns)]})
    print(prefix,json.dumps(records,ensure_ascii=False))
