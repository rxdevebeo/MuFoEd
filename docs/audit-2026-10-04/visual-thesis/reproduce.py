"""Read-only corpus census plus small independent layout probes."""
from pathlib import Path
from zipfile import ZipFile, ZIP_DEFLATED
from lxml import etree as E
from PIL import Image
from collections import Counter
import base64,hashlib,io,json,subprocess
ROOT=Path(__file__).resolve().parents[3]; OUT=Path(__file__).resolve().parent
SRC=ROOT/'strict-ooxml-core/tests/docx/Clio Der Sarkissian. - Mitochondrial DNA in Ancient Human Populations of Europe. - 2011.docx'
N={'w':'http://schemas.openxmlformats.org/wordprocessingml/2006/main'}
S={'s':'http://www.w3.org/2000/svg'}
result={'source_sha256':hashlib.sha256(SRC.read_bytes()).hexdigest()}
with ZipFile(SRC) as z:
    d=E.fromstring(z.read('word/document.xml'))
    result['source']={'sections':len(d.xpath('//w:sectPr',namespaces=N)),
                      'frame_occurrences':len(d.xpath('//w:framePr',namespaces=N)),
                      'frame_wraps':dict(Counter(d.xpath('//w:framePr/@w:wrap',namespaces=N))),
                      'tab_alignments':dict(Counter(d.xpath('//w:tabs/w:tab/@w:val',namespaces=N))),
                      'zero_margin_sections':len(d.xpath('//w:sectPr[w:pgMar[@w:left="0" and @w:right="0" and @w:top="0" and @w:bottom="0"]]',namespaces=N))}
    result['pages']={}
    for num in [2,54,56,104]:
        svg=E.parse(str(OUT/'original'/f'page-{num}.svg'))
        width=float(svg.getroot().get('width'))
        ts=svg.xpath('//s:text',namespaces=S)
        record={'page_width':width,'items':dict(Counter(E.QName(a).localname for a in svg.getroot())),
                'first_text_items':[{'text':a.text,'x':a.get('x'),'y':a.get('y')} for a in ts[:15]],
                'text_origins_outside_page':[{'text':a.text,'x':a.get('x'),'y':a.get('y')} for a in ts if float(a.get('x'))>width or float(a.get('x'))<0],
                'images':[]}
        for i,a in enumerate(svg.xpath('//s:image',namespaces=S)):
            data=base64.b64decode(a.get('{http://www.w3.org/1999/xlink}href').split(',',1)[1])
            matches=[name for name in z.namelist() if name.startswith('word/media/') and z.read(name)==data]
            im=Image.open(io.BytesIO(data)); im.load()
            record['images'].append({'source_parts':matches,'sha256':hashlib.sha256(data).hexdigest(),'decode':'complete',
                                     'pixels':im.size,'box':{k:a.get(k) for k in ['x','y','width','height']}})
        result['pages'][str(num)]=record
    result['toc_first_paragraph']={}
    for p in d.xpath('//w:body/w:p',namespaces=N):
        t=''.join(p.xpath('.//w:t/text()',namespaces=N))
        if t.startswith('TABLE OF CONTENTS'):
            result['toc_first_paragraph']={'text':t,'explicit_breaks':len(p.xpath('.//w:br|.//w:cr',namespaces=N)),
                                           'tabs':len(p.xpath('./w:r/w:tab',namespaces=N))}
            break
result['rendered_pages']=len(list((OUT/'original').glob('page-*.svg')))

RPR='<w:rPr><w:rFonts w:ascii="Times New Roman" w:hAnsi="Times New Roman" w:cs="Times New Roman"/><w:sz w:val="20"/></w:rPr>'
def run(text):return '<w:r>'+RPR+'<w:t xml:space="preserve">'+text+'</w:t></w:r>'
def probe(name,ppr,contents):
    file=OUT/(name+'.docx')
    doc='<w:document xmlns:w="'+N['w']+'"><w:body><w:p><w:pPr>'+ppr+'</w:pPr>'+contents+'</w:p><w:sectPr><w:pgSz w:w="9000" w:h="9000"/><w:pgMar w:top="0" w:right="0" w:bottom="0" w:left="0" w:header="0" w:footer="0"/></w:sectPr></w:body></w:document>'
    with ZipFile(file,'w',ZIP_DEFLATED) as z:
        z.writestr('[Content_Types].xml','<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/></Types>')
        z.writestr('_rels/.rels','<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>')
        z.writestr('word/document.xml',doc)
    proc=subprocess.run([str(ROOT/'target/debug/strict-ooxml.exe'),'render',str(file),'--transitional','--out',str(OUT/name)],capture_output=True)
    (OUT/(name+'.log')).write_bytes(proc.stdout+proc.stderr)
    svg=E.parse(str(OUT/name/'page-1.svg')); ts=svg.xpath('//s:text',namespaces=S)
    return {'exit':proc.returncode,'items':[{'text':a.text,'x':a.get('x'),'y':a.get('y')} for a in ts]}
result['probes']={
    'frame':probe('frame','<w:framePr w:wrap="none" w:hAnchor="page" w:vAnchor="page" w:x="3000" w:y="2000" w:w="3000" w:h="1000"/>',run('Framed text')),
    'frame-shift':probe('frame-shift','<w:framePr w:wrap="none" w:hAnchor="page" w:vAnchor="page" w:x="4500" w:y="4000" w:w="3000" w:h="1000"/>',run('Framed text')),
    'right-tab':probe('right-tab','<w:tabs><w:tab w:val="right" w:leader="dot" w:pos="4800"/></w:tabs>',run('Entry')+'<w:r><w:tab/></w:r>'+run('12')),
    'first-line':probe('first-line','<w:ind w:firstLine="2400"/>',run('word '*50))}
result['source_unchanged']=result['source_sha256']==hashlib.sha256(SRC.read_bytes()).hexdigest()
(OUT/'evidence.json').write_text(json.dumps(result,ensure_ascii=False,indent=2),encoding='utf-8')
print(json.dumps({'source':result['source'],'pages':result['rendered_pages'],
                  'source_unchanged':result['source_unchanged'],
                  'probes':{k:dict(exit=v['exit'],first=v['items'][:3],max_x=max(float(a['x']) for a in v['items'])) for k,v in result['probes'].items()},
                  'outside_origins':{k:len(v['text_origins_outside_page']) for k,v in result['pages'].items()},
                  'images104':result['pages']['104']['images']},ensure_ascii=False,indent=2))
