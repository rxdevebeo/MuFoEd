# strict-ooxml-html

UTF-8 HTML fragment import into the existing StrictLib WML model. This crate
owns imported image bytes and has no network, filesystem, clipboard or UI access.
The `strict-ooxml` facade exposes it through the optional `html` feature and
`StrictDocument::import_html`, which uses the shared Strict package writer.

Supported subset:

- paragraphs, headings, explicit line breaks and preformatted text;
- bold, italic, underline, strikeout, superscript/subscript, font family/size/color;
- inline CSS for these properties, paragraph alignment and indents;
- ordered/unordered lists with structural numbering;
- tables, nested tables and horizontal cell spans;
- PNG, JPEG, GIF and BMP images from data URLs or a caller-provided resolver.

Relative image URLs use `HtmlOptions::base_url`. Resolvers must bound their I/O;
the importer checks payload sizes independently. Budgets cover input size, node
count, nesting, image attempts, total retained image bytes and image pixels.
Unavailable/unsupported images produce an alternative-text placeholder and a
named issue. Resource failures, including byte limits, are recoverable issues;
input/tree/image-count/pixel budget failures abort without a partial result.

This is a document importer, not a browser layout engine. CSS stylesheets,
JavaScript, floating layout, vertical cell spans and interactive content are
not implemented. Link text is retained but link targets are reported as lost.
HTML tokenization uses html5ever; fragment block recovery is intentionally
limited and does not implement the complete HTML5 tree-construction algorithm.
Inspect `HtmlReport` as well as the facade's writer report. Named issues cover
recognized unsupported constructs; the report is not an exhaustive HTML/CSS
conformance assessment.
