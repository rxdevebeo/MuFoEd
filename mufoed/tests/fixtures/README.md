`ole-equation.docx` is a synthetic Transitional DOCX for editor regressions.
It contains `left`, a `w:object` (`ProgID="Equation.3"`) with a small PNG cached
preview, `right`, and a second paragraph `target`. The 16 zero bytes in the
embedded payload are inert fixture data, not a functioning Equation object.
The test exercises the real Transitional normalizer and its documented OLE
payload loss, then picture selection, resizing, wrapping, movement, OCR review,
OMML replacement, save/reopen, and Undo/Redo. OCR replies come from a loopback
test server; this fixture makes no claim about real-model recognition accuracy.

`display-formula.docx` retains the first display equation from the user-supplied
`МК-2503 Ленников Статья.docx`, with the article prose removed. It checks
`m:oMathPara` selection, editing, resizing, movement, deletion, save/reopen,
and Undo. Set `MUFOED_FORMULA_TEST_FILE` to run that regression against the
complete source article instead. The original file is only read.
