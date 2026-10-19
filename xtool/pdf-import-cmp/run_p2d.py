import sys, time, logging, pathlib
logging.disable(logging.WARNING)
from pdf2docx import Converter
src, out = pathlib.Path(sys.argv[1]), pathlib.Path(sys.argv[2])
for p in sorted(src.glob("*.pdf")):
    t=time.time()
    try:
        cv=Converter(str(p)); cv.convert(str(out/(p.stem+".docx")), start=0, end=6); cv.close()
        print(p.stem, "ok", f"{(time.time()-t)*1000:.0f}ms", flush=True)
    except Exception as e:
        print(p.stem, "ERR", type(e).__name__, e, flush=True)
