from pathlib import Path
from zipfile import ZipFile
from lxml import etree as E
from fontTools.ttLib import TTFont
import base64,json,hashlib,subprocess,copy
ROOT=Path(__file__).resolve().parents[3]; OUT=Path(__file__).resolve().parent
N={'w':'http://schemas.openxmlformats.org/wordprocessingml/2006/main','wp':'http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing','a':'http://schemas.openxmlformats.org/drawingml/2006/main','r':'http://schemas.openxmlformats.org/officeDocument/2006/relationships'}
S={'s':'http://www.w3.org/2000/svg'}; W='{'+N['w']+'}'
ev=json.loads((OUT/'evidence.json').read_text(encoding='utf-8')); result={}
svg=E.parse(str(OUT/'book/page-31.svg')); im=svg.xpath('//s:image',namespaces=S)[0]
data=base64.b64decode(im.get('{http://www.w3.org/1999/xlink}href').split(',',1)[1]); box={k:float(im.get(k)) for k in ['x','y','width','height']}
inside=[a for a in svg.xpath('//s:text',namespaces=S) if box['x']<float(a.get('x'))<box['x']+box['width'] and box['y']<float(a.get('y'))<box['y']+box['height']]
result['book']={'image_box':box,'text_origin_baselines_inside_image':len(inside),'examples':[dict(text=a.text,x=a.get('x'),y=a.get('y')) for a in inside[:8]]}
src=ROOT/'strict-ooxml-core/tests/docx'/ev['book']['source']
with ZipFile(src) as z:
    part=next(a for a in z.namelist() if a.startswith('word/media/') and z.read(a)==data)
    rel=E.fromstring(z.read('word/_rels/document.xml.rels'))
    rid=next(a.get('Id') for a in rel if a.get('Target').replace('../','').lstrip('/') in [part,part.removeprefix('word/')])
    d=E.fromstring(z.read('word/document.xml'))
    anchor=next(a for a in d.xpath('//wp:anchor',namespaces=N) if a.xpath('.//a:blip/@r:embed',namespaces=N)==[rid])
    result['book']['source_part']=part; result['book']['anchor_attributes']=dict(anchor.attrib)
    result['book']['wrap']=[E.tostring(a,encoding='unicode') for a in anchor if 'wrap' in E.QName(a).localname]
    host=anchor
    while host.getparent().tag!=W+'body':host=host.getparent()
    body=d.find('w:body',N); children=list(body); idx=children.index(host); keep=children[idx:idx+5]+[children[-1]]
    for a in children:
        if a not in keep:body.remove(a)
    records={}
    for mode in ['square','none']:
        tree=copy.deepcopy(d)
        if mode=='none':
            a=tree.xpath('//wp:anchor',namespaces=N)[0]
            for b in list(a):
                if 'wrap' in E.QName(b).localname:a.replace(b,E.Element('{'+N['wp']+'}wrapNone'))
        file=OUT/('portrait-'+mode+'.docx')
        with ZipFile(file,'w') as dst:
            for entry in z.infolist():dst.writestr(copy.copy(entry),E.tostring(tree,xml_declaration=True,encoding='UTF-8') if entry.filename=='word/document.xml' else z.read(entry.filename))
        proc=subprocess.run([str(ROOT/'target/debug/strict-ooxml.exe'),'render',str(file),'--transitional','--out',str(OUT/('portrait-'+mode))],capture_output=True)
        (OUT/('portrait-'+mode+'.log')).write_bytes(proc.stdout+proc.stderr)
        records[mode]=[(p.name,hashlib.sha256(p.read_bytes()).hexdigest()) for p in sorted((OUT/('portrait-'+mode)).glob('page-*.svg'))]
    result['book']['square_vs_none_svg_identical']=records['square']==records['none']; result['book']['probe_pages']=records
result['marketing']={}
texts=ev['marketing']['svg']['1']['texts'][:4]
for face,path in [('Carlito',ROOT/'strict-ooxml-render-svg/assets/fonts/carlito/Carlito-Regular.ttf'),('Times New Roman',Path('C:/Windows/Fonts/times.ttf'))]:
    f=TTFont(path); cmap=f.getBestCmap(); h=f['hmtx'].metrics; up=f['head'].unitsPerEm
    result['marketing'][face]=[{'text':a['text'],'advance_hmtx':sum(h[cmap[ord(c)]][0] for c in a['text'])/up*float(a['font-size']),'allocated':float(texts[i+1]['x'])-float(a['x'])} for i,a in enumerate(texts[:-1])]
(OUT/'probes.json').write_text(json.dumps(result,ensure_ascii=False,indent=2),encoding='utf-8');print(json.dumps(result,ensure_ascii=False,indent=2))
