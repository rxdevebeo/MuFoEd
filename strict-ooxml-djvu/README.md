# strict-ooxml-djvu

Bounded native DjVu reading for Strict OOXML import. Text layers are parsed
by this crate; MIT `djvu-rs` component codecs handle BZZ/JB2/IW44. Optional
`djvu-image` enables page PNGs. No external DjVuLibre process is invoked by
production code. No DjVu document writer is provided.

“DjVu” is used descriptively to identify the supported format. This project
is not affiliated with, endorsed by, or a product of the format's trademark owners.

See [import and OCR guide](../docs/DJVU_IMPORT.md),
[provenance](PROVENANCE.md), and [validation](../docs/djvu-validation/README.md).

Image decoding also supports FGbz palettes, JPEG planes, ordinary Smmr and
standalone PM44/BM44. Striped Smmr is explicitly refused. Physical legacy
page size assumes 100 dpi because those files carry no INFO. See
[decoder oracle](../docs/djvu-validation/decoder-oracle.json).
