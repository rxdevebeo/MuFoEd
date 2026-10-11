import glob,json,re,zipfile
from lxml import etree as E
ns={'m':'http://schemas.openxmlformats.org/officeDocument/2006/math','w':'http://schemas.openxmlformats.org/wordprocessingml/2006/main'}
for prefix in ['018','019','073']:
    root=E.fromstring(zipfile.ZipFile(glob.glob('testdata/CC0_DOCX_MATH/'+prefix+'*')[0]).read('word/document.xml'))
    if prefix=='019':
        for p in root.findall('.//w:p',ns):
            if p.find('.//w:tab',ns) is not None and p.find('.//w:instrText',ns) is not None:
                print(prefix,E.tostring(p,encoding='unicode')[:1000])
        continue
    expressions=[]
    for expr in root.findall('.//m:oMath',ns):
        text=''.join(expr.xpath('.//m:t/text()',namespaces=ns))
        expressions.append((len(text),text,[E.QName(c).localname for c in expr]))
    print(prefix,'longest OMML',sorted(expressions,reverse=True)[:6])
    if prefix=='018':
        expr=root.findall('.//m:oMath',ns)[0]
        print('018 first math tail',E.tostring(expr,encoding='unicode')[-4000:])
    with open('render_out/math-layout-audit/ours/'+prefix+'/text-pages.jsonl',encoding='utf8') as f:
        for line in f:
            page=json.loads(line)
            for s in page['segments']:
                if max(map(float,re.search('x="([^"]+)"',s['attributes'])[1].split()))>750:
                    print(prefix,'off-page text',page['page'],s)
