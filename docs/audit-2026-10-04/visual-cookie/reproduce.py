from pathlib import Path
from zipfile import ZipFile
from lxml import etree as E
import subprocess, json, hashlib
ROOT=Path(__file__).resolve().parents[3]
OUT=Path(__file__).resolve().parent
SRC=ROOT/'strict-ooxml-core/tests/docx/COOKIE POLICY.docx'
N={'w':'http://schemas.openxmlformats.org/wordprocessingml/2006/main'}
S={'s':'http://www.w3.org/2000/svg'}

def edit(name):
    dest=OUT/(name+'.docx')
    with ZipFile(SRC) as z, ZipFile(dest,'w') as dst:
        for entry in z.infolist():
            data=z.read(entry.filename)
            if entry.filename=='word/document.xml':
                d=E.fromstring(data)
                if name=='inherit-list-indent':
                    nodes=d.xpath('//w:p[w:pPr/w:numPr]/w:pPr/w:ind',namespaces=N)
                else:
                    nodes=d.xpath('//w:r[w:t="__________________"]/w:rPr/w:u',namespaces=N)
                for node in nodes:node.getparent().remove(node)
                data=E.tostring(d,encoding='UTF-8',xml_declaration=True,standalone=True)
            dst.writestr(entry,data)
    return dest

result={'source_sha256':hashlib.sha256(SRC.read_bytes()).hexdigest(),'cases':{}}
for name,file in [('original',SRC)]+[(n,edit(n)) for n in ['inherit-list-indent','no-underscore-decoration']]:
    folder=OUT/name
    proc=subprocess.run([str(ROOT/'target/debug/strict-ooxml.exe'),'render',str(file),'--transitional','--out',str(folder)],capture_output=True)
    (OUT/(name+'.log')).write_bytes(proc.stdout+proc.stderr)
    item={'exit':proc.returncode,'pages':len(list(folder.glob('*.svg'))),'lists':[],'underscore':[]}
    for page in sorted(folder.glob('*.svg')):
        ts=E.parse(str(page)).xpath('//s:text',namespaces=S)
        for i,t in enumerate(ts):
            if t.text and set(t.text)=={'_'}:
                item['underscore'].append({'text':t.text,**dict(t.attrib)})
            if t.text and t.text.startswith(('Chrome:','Explorer:','Safari:','Firefox:','Opera:')):
                prev=ts[i-1]
                item['lists'].append({'page':page.name,'text':t.text,'text_x':t.get('x'),'text_y':t.get('y'),
                                      'marker':prev.text,'marker_x':prev.get('x'),'marker_y':prev.get('y'),'marker_font':prev.get('font-family')})
    result['cases'][name]=item
result['source_unchanged']=result['source_sha256']==hashlib.sha256(SRC.read_bytes()).hexdigest()
(OUT/'evidence.json').write_text(json.dumps(result,ensure_ascii=False,indent=2),encoding='utf-8')
print(json.dumps(result,ensure_ascii=False,indent=2))
