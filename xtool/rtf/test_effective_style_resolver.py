"""Mutation controls for the independent effective-style resolver."""
import tempfile
from pathlib import Path
import unittest
import zipfile

from effective_style_resolver import diff, effective

NS = "http://purl.oclc.org/ooxml/wordprocessingml/main"


def package(document_body, styles_body):
    directory = tempfile.TemporaryDirectory()
    path = Path(directory.name) / "input.docx"
    with zipfile.ZipFile(path, "w") as archive:
        archive.writestr(
            "word/document.xml",
            f'<w:document xmlns:w="{NS}"><w:body>{document_body}</w:body></w:document>',
        )
        archive.writestr(
            "word/styles.xml",
            f'<w:styles xmlns:w="{NS}">{styles_body}</w:styles>',
        )
    return path, directory


def run(body, rpr=""):
    return f"<w:p><w:r>{rpr}<w:t>{body}</w:t></w:r></w:p>"


class EffectiveResolver(unittest.TestCase):
    def test_rfonts_attributes_inherit_from_doc_defaults(self):
        # A later `<w:rFonts w:hint>` must not wipe the inherited ascii slot, so
        # this run resolves to the same face as one with no `w:rPr` at all.
        defaults = '<w:docDefaults><w:rPrDefault><w:rPr><w:rFonts w:ascii="Calibri" w:hAnsi="Calibri"/></w:rPr></w:rPrDefault></w:docDefaults>'
        hinted, hinted_dir = package(run("A", "<w:rPr><w:rFonts w:hint=\"default\"/></w:rPr>"), defaults)
        self.addCleanup(hinted_dir.cleanup)
        plain, plain_dir = package(run("A"), defaults)
        self.addCleanup(plain_dir.cleanup)
        self.assertEqual(diff(effective(hinted), effective(plain)), [])

    def test_explicit_strict_default_equals_absence(self):
        defaults = '<w:docDefaults><w:rPrDefault><w:rPr/></w:rPrDefault></w:docDefaults>'
        explicit = (
            '<w:pPr><w:jc w:val="start"/><w:bidi w:val="false"/>'
            '<w:widowControl w:val="true"/><w:ind w:start="0" w:end="0"/></w:pPr>'
        )
        with_defaults, with_dir = package(f"<w:p>{explicit}<w:r><w:t>A</w:t></w:r></w:p>", defaults)
        self.addCleanup(with_dir.cleanup)
        without, without_dir = package(run("A"), defaults)
        self.addCleanup(without_dir.cleanup)
        self.assertEqual(diff(effective(with_defaults), effective(without)), [])

    def test_non_default_toggle_is_still_reported(self):
        defaults = '<w:docDefaults><w:rPrDefault><w:rPr/></w:rPrDefault></w:docDefaults>'
        bold, bold_dir = package(run("A", "<w:rPr><w:b w:val=\"true\"/></w:rPr>"), defaults)
        self.addCleanup(bold_dir.cleanup)
        plain, plain_dir = package(run("A"), defaults)
        self.addCleanup(plain_dir.cleanup)
        differences = diff(effective(bold), effective(plain))
        self.assertEqual([entry["prop"] for entry in differences], ["b"])


if __name__ == "__main__":
    unittest.main()
