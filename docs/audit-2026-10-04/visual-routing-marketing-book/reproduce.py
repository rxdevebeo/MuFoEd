from pathlib import Path
from zipfile import ZipFile
from lxml import etree as E
from collections import Counter
from concurrent.futures import ThreadPoolExecutor
import json,hashlib,subprocess
ROOT=Path(__file__).resolve().parents[3]; OUT=Path(__file__).resolve().parent
N={'w':'http://schemas.openxmlformats.org/wordprocessingml/2006/main','wp':'http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing'}
S={'s':'http://www.w3.org/2000/svg'}
FILES={'routing':'Design for Rectilinear Edge Routing.docx','marketing':'Digital_Marketing_Service_Agreement.docx','book':'Programming-Basics-CSharp-Book-and-Video-Lessons-Nakov-v2019.docx'}
def inspect(pair):
    key,name=pair; src=ROOT/'strict-ooxml-core/tests/docx'/name
    sha=hashlib.sha256(src.read_bytes()).hexdigest()
    with ZipFile(src) as z:
        d=E.fromstring(z.read('word/document.xml')); styles=E.fromstring(z.read('word/styles.xml'))
        result={'source':name,'sha256':sha,'frames':len(d.xpath('//w:framePr',namespaces=N)),'anchors':len(d.xpath('//wp:anchor',namespaces=N)),
        'tabs':dict(Counter(d.xpath('//w:tabs/w:tab/@w:val',namespaces=N))),
        'fonts':dict(Counter(d.xpath('//w:rFonts/@w:ascii|//w:rFonts/@w:cs',namespaces=N))),
        'paragraphs':[],'anchor_xml':[E.tostring(a,encoding='unicode') for a in d.xpath('//wp:anchor',namespaces=N)],
        'style_xml':E.tostring(styles,encoding='unicode')}
        for i,p in enumerate(d.xpath('//w:body/w:p|//w:body/w:sdt/w:sdtContent/w:p',namespaces=N)):
            text=''.join(p.xpath('.//w:t/text()',namespaces=N))
            if key!='book' and (i<90 or 'Contents' in text): result['paragraphs'].append({'index':i,'text':text,'xml':E.tostring(p,encoding='unicode')})
    proc=subprocess.run([str(ROOT/'target/debug/strict-ooxml.exe'),'render',str(src),'--transitional','--out',str(OUT/key)],capture_output=True)
    (OUT/(key+'.log')).write_bytes(proc.stdout+proc.stderr)
    result['exit']=proc.returncode; pages=list((OUT/key).glob('page-*.svg')); result['pages']=len(pages)
    result['svg']={}
    for num in ([1,2,3] if key!='book' else [30,31,32]):
        f=OUT/key/f'page-{num}.svg'
        if not f.exists():continue
        svg=E.parse(str(f)); result['svg'][str(num)]={'texts':[dict(a.attrib,text=a.text) for a in svg.xpath('//s:text',namespaces=S)],'images':[{k:a.get(k) for k in ['x','y','width','height']} for a in svg.xpath('//s:image',namespaces=S)]}
    assert hashlib.sha256(src.read_bytes()).hexdigest()==sha
    return key,result
result=dict(ThreadPoolExecutor(3).map(inspect,FILES.items()))
(OUT/'evidence.json').write_text(json.dumps(result,ensure_ascii=False,indent=2),encoding='utf-8')
print(json.dumps({k:{a:v[a] for a in ['sha256','frames','anchors','tabs','fonts','exit','pages']} for k,v in result.items()},ensure_ascii=False,indent=2))
