# strict-ooxml-djvu

Bounded native DjVu reading for Strict OOXML import. Text layers are parsed
by this crate; MIT `djvu-rs` component codecs handle BZZ/JB2/IW44. Optional
`djvu-image` enables page PNGs. No external DjVuLibre process is invoked by
production code. No DjVu document writer is provided.

“DjVu” is used descriptively to identify the supported format. This project
is not affiliated with, endorsed by, or a product of the format's trademark owners.

See [import and OCR guide](../docs/DJVU_IMPORT.md),
[provenance](PROVENANCE.md), and [validation](../docs/djvu-validation/README.md).
