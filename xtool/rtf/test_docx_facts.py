"""Known XML constants prove text view and structural boundaries independently."""

import tempfile
from pathlib import Path
import unittest
import xml.etree.ElementTree as ET
import zipfile

from docx_facts import inspect, paragraph_records, paragraph_text

PREFIX = '<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body>'
SUFFIX = '</w:body></w:document>'


class DocxFactsTests(unittest.TestCase):
    def test_properties_are_not_visible_text(self):
        xml = PREFIX + '<w:p><w:pPr><w:tabs><w:tab w:pos="720"/></w:tabs></w:pPr>' \
            '<w:r><w:rPr><w:tab/></w:rPr><w:t>A</w:t><w:tab/><w:t>B</w:t></w:r></w:p>' + SUFFIX
        self.assertEqual(list(paragraph_records(ET.fromstring(xml)))[0]["text"], "A\tB")

    def test_exact_text_controls_and_final_view(self):
        xml = PREFIX + '<w:p><w:r><w:t>A </w:t><w:tab/><w:t>Ж</w:t><w:br/><w:noBreakHyphen/><w:softHyphen/>' \
            '<w:br w:type="page"/></w:r><w:del><w:r><w:delText>old</w:delText></w:r></w:del>' \
            '<w:ins><w:r><w:t>new</w:t></w:r></w:ins><w:r><w:instrText>instruction</w:instrText></w:r></w:p>' + SUFFIX
        root = ET.fromstring(xml)
        self.assertEqual(list(paragraph_records(root))[0]["text"], "A \tЖ\n\u2011\u00ad\fnew")

    def test_textbox_is_not_duplicated_in_body(self):
        xml = PREFIX + '<w:p><w:r><w:t>body</w:t><w:drawing><w:txbxContent><w:p><w:r><w:t>box</w:t>' \
            '</w:r></w:p></w:txbxContent></w:drawing></w:r></w:p>' + SUFFIX
        self.assertEqual(list(paragraph_records(ET.fromstring(xml)))[0]["text"], "body")

    def test_table_cell_paragraph_paths_preserved(self):
        xml = PREFIX + '<w:tbl><w:tr><w:tc><w:p><w:r><w:t>cell1</w:t></w:r></w:p></w:tc>' \
            '<w:tc><w:p><w:r><w:t>cell2</w:t></w:r></w:p></w:tc></w:tr></w:tbl>' + SUFFIX
        records = list(paragraph_records(ET.fromstring(xml)))
        self.assertEqual([row["text"] for row in records], ["cell1", "cell2"])
        self.assertIn("tc[0]", records[0]["path"])
        self.assertIn("tc[1]", records[1]["path"])

    def test_media_hash_and_region(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "fixture.docx"
            with zipfile.ZipFile(path, "w") as archive:
                archive.writestr("word/document.xml", PREFIX + '<w:p><w:r><w:t>exact</w:t></w:r></w:p>' + SUFFIX)
                archive.writestr("[Content_Types].xml", '<Types/>')
                archive.writestr("word/media/image.bin", b"abc")
            facts = inspect(path)
            self.assertEqual(facts["regions"][0]["paragraphs"][0]["text"], "exact")
            self.assertEqual(facts["media"][0]["sha256"], "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad")


if __name__ == "__main__":
    unittest.main()
