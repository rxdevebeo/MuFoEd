"""Mutation controls for the independent structural facts reader."""
import tempfile
from pathlib import Path
import unittest
import zipfile

from structure_oracle import inspect


class StructuralFacts(unittest.TestCase):
    def package(self, body, notes=None, namespace="http://purl.oclc.org/ooxml/wordprocessingml/main"):
        directory = tempfile.TemporaryDirectory()
        self.addCleanup(directory.cleanup)
        path = Path(directory.name) / "input.docx"
        with zipfile.ZipFile(path, "w") as archive:
            archive.writestr("word/document.xml", f'<w:document xmlns:w="{namespace}"><w:body>{body}</w:body></w:document>')
            if notes is not None:
                archive.writestr("word/footnotes.xml", f'<w:footnotes xmlns:w="{namespace}">{notes}</w:footnotes>')
        return path

    def test_property_tabs_never_become_visible_text(self):
        path = self.package('<w:p><w:pPr><w:tabs><w:tab w:val="right" w:pos="720"/></w:tabs></w:pPr><w:r><w:t>A</w:t><w:tab/><w:t>B</w:t></w:r></w:p>')
        facts = inspect(path)
        self.assertEqual(facts["body"], ["A\tB"])
        self.assertEqual(facts["paragraph_properties"][0]["tabs"][0]["tab"]["val"], "end")

    def test_normal_note_zero_is_not_a_separator(self):
        notes = '<w:footnote w:id="2" w:type="separator"><w:p/></w:footnote><w:footnote w:id="0"><w:p><w:r><w:t>Note</w:t></w:r></w:p></w:footnote>'
        path = self.package('<w:p><w:r><w:t>A</w:t></w:r><w:r><w:footnoteReference w:id="0"/></w:r></w:p>', notes)
        facts = inspect(path)
        self.assertEqual(facts["notes"], {"footnote": [["Note"]]})
        self.assertEqual(facts["note_references"][0]["note"], ["Note"])
        self.assertEqual(facts["note_references"][0]["preceding_text"], "A")

    def test_dropped_field_instruction_is_detected_with_equal_body(self):
        valid = inspect(self.package('<w:p><w:fldSimple w:instr="PAGE"><w:r><w:t>1</w:t></w:r></w:fldSimple></w:p>'))
        mutated = inspect(self.package('<w:p><w:r><w:t>1</w:t></w:r></w:p>'))
        self.assertEqual(valid["body"], mutated["body"])
        self.assertNotEqual(valid["fields"], mutated["fields"])

    def test_dropped_merge_is_detected_with_equal_body(self):
        xml = '<w:tbl><w:tr><w:tc><w:tcPr><w:vMerge w:val="restart"/></w:tcPr><w:p><w:r><w:t>A</w:t></w:r></w:p></w:tc></w:tr></w:tbl>'
        valid = inspect(self.package(xml))
        mutated = inspect(self.package(xml.replace('<w:vMerge w:val="restart"/>', '')))
        self.assertEqual(valid["body"], mutated["body"])
        self.assertNotEqual(valid["tables"], mutated["tables"])


if __name__ == "__main__":
    unittest.main()
