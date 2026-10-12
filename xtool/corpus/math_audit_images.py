"""Make comparison sheets from the locally rendered audit pages."""
from pathlib import Path
from PIL import Image, ImageDraw
ROOT = Path('render_out/math-layout-audit')
for prefix in ['019','007','081']:
    files = [ROOT/'reference'/f'{prefix}-p1.png',
             ROOT/'ours'/prefix/'page-1-after.png']
    if prefix == '019': files.insert(1,ROOT/'ours'/prefix/'page-1-before.png')
    images = [Image.open(f).convert('RGB') for f in files]
    width = 480
    resized = [im.resize((width,round(im.height*width/im.width))) for im in images]
    result = Image.new('RGB',(width*len(images),max(im.height for im in resized)+30),'white')
    draw = ImageDraw.Draw(result)
    labels = ['WPS reference','StrictLib after'] if len(images)==2 else ['WPS reference','StrictLib before','StrictLib after']
    for i,(im,label) in enumerate(zip(resized,labels)):
        result.paste(im,(i*width,30)); draw.text((i*width+12,8),label,fill='black')
    result.save(ROOT/f'{prefix}-comparison.png')
