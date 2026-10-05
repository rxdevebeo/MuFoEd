"""Measure merged-cell continuation and verify the page-8 PNG resource."""
from pathlib import Path
from zipfile import ZipFile
from lxml import etree as E
from PIL import Image
import subprocess,base64,hashlib,io,json,copy
ROOT=Path(__file__).resolve().parents[3]; OUT=Path(__file__).resolve().parent
SRC=ROOT/'strict-ooxml-core/tests/docx/3. Simple-Conditions.docx'
N={'w':'http://schemas.openxmlformats.org/wordprocessingml/2006/main'}
S={'s':'http://www.w3.org/2000/svg'}
result={'source_sha256':hashlib.sha256(SRC.read_bytes()).hexdigest()}
variant=OUT/'no-vertical-merge.docx'
with ZipFile(SRC) as z, ZipFile(variant,'w') as dst:
    document=E.fromstring(z.read('word/document.xml'))
    table=document.xpath('//w:tbl',namespaces=N)[0]
    result['table_grid_twips']=table.xpath('./w:tblGrid/w:gridCol/@w:w',namespaces=N)
    result['right_cell_merges']=[{'row':i,'merge':c.find('w:tcPr/w:vMerge',N).get('{'+N['w']+'}val','continue') if c.find('w:tcPr/w:vMerge',N) is not None else None}
                                for i,r in enumerate(table.findall('w:tr',N)) for c in r.findall('w:tc',N)[-1:]]
    for node in table.xpath('.//w:vMerge',namespaces=N):node.getparent().remove(node)
    for entry in z.infolist():
        data=E.tostring(document,encoding='UTF-8',xml_declaration=True,standalone=True) if entry.filename=='word/document.xml' else z.read(entry.filename)
        dst.writestr(copy.copy(entry),data)
    svg=E.parse(str(OUT/'original/page-8.svg'))
    im=svg.xpath('//s:image',namespaces=S)[0]
    data=base64.b64decode(im.get('{http://www.w3.org/1999/xlink}href').split(',',1)[1])
    matches=[p for p in z.namelist() if p.startswith('word/media/') and z.read(p)==data]
    image=Image.open(io.BytesIO(data));image.load()
    (OUT/'page8-source.png').write_bytes(data)
    result['page8_image']={'source_parts':matches,'bytes':len(data),'sha256':hashlib.sha256(data).hexdigest(),
                         'format':image.format,'pixels':image.size,'decode':'complete',
                         'svg_box':{k:im.get(k) for k in ['x','y','width','height']}}
    result['page8_caption']=''.join(svg.xpath('//s:text/text()',namespaces=S))[:320]

proc=subprocess.run([str(ROOT/'target/debug/strict-ooxml.exe'),'render',str(variant),'--transitional','--out',str(OUT/'no-vertical-merge')],capture_output=True)
(OUT/'no-vertical-merge.log').write_bytes(proc.stdout+proc.stderr)
result['variant_exit']=proc.returncode
result['cases']={}
for name in ['original','no-vertical-merge']:
    case={}
    for page in [3,4]:
        d=E.parse(str(OUT/name/f'page-{page}.svg'))
        rects=[{k:float(a.get(k)) for k in ['x','y','width','height']} for a in d.xpath('//s:rect',namespaces=S)
               if a.get('fill')!='#ffffff' or float(a.get('x','0'))!=0]
        if page==4:rects=[a for a in rects if a['y']<110]
        else:rects=[a for a in rects if a['y']>700]
        case[str(page)]={'cell_rectangles':rects,'page_height':float(d.getroot().get('height'))}
    result['cases'][name]=case
result['source_unchanged']=result['source_sha256']==hashlib.sha256(SRC.read_bytes()).hexdigest()
(OUT/'evidence.json').write_text(json.dumps(result,ensure_ascii=False,indent=2),encoding='utf-8')
print(json.dumps(result,ensure_ascii=False,indent=2))
