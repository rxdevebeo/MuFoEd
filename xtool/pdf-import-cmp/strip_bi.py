import sys, re, pymupdf
d = pymupdf.open(sys.argv[1]); mode = sys.argv[3]
for pg in d:
    for x in pg.get_contents():
        c = d.xref_stream(x)
        if mode == "drop": c = re.sub(rb"\sBI\s.*?\sID\s.*?\sEI(?=\s)", b" ", c, flags=re.S)
        elif mode == "cs": c = c.replace(b"/IM true", b"/IM true /CS /G")  # no-op probe
        d.update_stream(x, c)
d.save(sys.argv[2])
