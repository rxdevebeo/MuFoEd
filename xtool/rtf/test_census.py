"""Independent assertions on small byte fixtures, not on census internals."""

import hashlib
import json
from pathlib import Path
import tempfile
import unittest

from census import expectation_template, inventory, scan, write_json


class ScannerTests(unittest.TestCase):
    def test_binary_masks_syntax(self):
        result = scan(b"{\\rtf1\\bin8 {\\pict}}x}")
        self.assertEqual(result["status"], "Pass")
        self.assertEqual(result["binary_bytes"], 8)
        self.assertEqual(result["groups"], 1)
        self.assertNotIn("pict", result["control_words"])

    def test_escaped_braces_are_not_groups(self):
        result = scan(b"{\\rtf1 \\{\\}\\\\\\'7b}")
        self.assertEqual(result["status"], "Pass")
        self.assertEqual(result["groups"], 1)
        self.assertEqual(result["hex_escapes"], 1)

    def test_group_first_and_ignorable(self):
        result = scan(b"{\\rtf1{\\*\\unknown payload}{\\fonttbl{\\f0 Arial;}}}")
        self.assertEqual(result["status"], "Pass")
        self.assertEqual(result["ignorable_group_first_words"], {"unknown": 1})
        self.assertEqual(result["recognized_destination_groups"], {"fonttbl": 1})
        self.assertEqual(result["group_first_control_words"]["f"], 1)

    def test_signed_parameters_and_uc(self):
        result = scan(b"{\\rtf1\\ansicpg1251\\uc0\\u-10179\\u-8704}")
        self.assertEqual(result["status"], "Pass")
        self.assertEqual(result["parameters"]["ansicpg"], {"1251": 1})
        self.assertEqual(result["parameters"]["uc"], {"0": 1})
        self.assertEqual(result["control_words"]["u"], 2)

    def test_header_without_a_version(self):
        self.assertEqual(scan(b"{\\rtf \\ansi x}")["status"], "Pass")
        self.assertEqual(scan(b"{\\rtfx}")["errors"][0]["id"], "invalid_header")

    def test_lone_minus_is_following_text(self):
        result = scan(b"{\\rtf1\\b-x}")
        self.assertEqual(result["status"], "Pass")
        self.assertEqual(result["text_payload_bytes"], 2)

    def test_fatal_errors_are_partial(self):
        cases = {
            b"plain": "invalid_header",
            b"{\\rtf1": "unclosed_group",
            b"{\\rtf1\\'xz}": "invalid_hex_escape",
            b"{\\rtf1\\bin-1 a}": "invalid_binary_length",
            b"{\\rtf1\\bin99 a}": "truncated_binary",
            b"{\\rtf1}x": "trailing_content",
            b"{\\rtf1\\fs2147483648}": "parameter_overflow",
            b"{\\rtf1\\aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa}": "control_word_limit",
        }
        for data, code in cases.items():
            with self.subTest(data=data):
                result = scan(data)
                self.assertEqual(result["status"], "Failed")
                self.assertFalse(result["complete"])
                self.assertEqual(result["errors"][0]["id"], code)

    def test_depth_and_token_budget(self):
        self.assertEqual(scan(b"{\\rtf1{\\b x}}", max_depth=1)["errors"][0]["id"], "depth_limit")
        self.assertEqual(scan(b"{\\rtf1 x}", max_tokens=1)["errors"][0]["id"], "token_limit")

    def test_presence_is_not_semantic_support(self):
        result = scan(b"{\\rtf1{\\*\\private \\trowd \\pict}}")
        self.assertIn("pictures", result["feature_hits"])
        self.assertEqual(result["claim"], "bounded_lexical_scan_only")

    def test_nul_trailer_is_candidate_not_unnoticed_loss(self):
        result = scan(b"{\\rtf1 x}\r\n\0")
        self.assertEqual(result["status"], "Pass")
        self.assertEqual(result["warnings"][0]["id"], "nul_trailer_candidate")
        self.assertEqual(result["warnings"][0]["bytes"], 1)
        self.assertEqual(result["scanned_bytes"], 12)
        self.assertEqual(scan(b"{\\rtf1}\0bad")["errors"][0]["id"], "trailing_content")

    def test_itap_one_does_not_prove_nested_table(self):
        self.assertNotIn("nested_tables", scan(b"{\\rtf1\\itap1 x}")["feature_hits"])
        self.assertIn("nested_tables", scan(b"{\\rtf1\\itap2 x}")["feature_hits"])


class InventoryTests(unittest.TestCase):
    def make_corpus(self, root):
        entries = []
        for index in range(100):
            data = b"{\\rtf1 text}"
            name = f"{index:03}.rtf"
            (root / name).write_bytes(data)
            entries.append({"filename": name, "sha256": hashlib.sha256(data).hexdigest(), "byte_size": len(data)})
        (root / "manifest.json").write_text(json.dumps(entries), encoding="utf-8")

    def test_full_manifest_and_pending_oracles(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            self.make_corpus(root)
            result = inventory(root)
            self.assertEqual(result["summary"]["integrity_pass"], 100)
            self.assertEqual(result["summary"]["lexical_pass"], 100)
            template = expectation_template(result)
            self.assertEqual(len(template["files"]), 100)
            self.assertIsNone(template["files"][0]["expected_import_outcome"])
            self.assertFalse(template["files"][0]["reviewed"])

    def test_missing_changed_extra_do_not_shrink_denominator(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            self.make_corpus(root)
            (root / "000.rtf").unlink()
            (root / "001.rtf").write_bytes(b"{\\rtf1 changed}")
            (root / "extra.rtf").write_bytes(b"{\\rtf1}")
            result = inventory(root)
            self.assertEqual(len(result["files"]), 100)
            self.assertEqual(result["summary"]["integrity_pass"], 98)
            self.assertTrue(result["global_errors"])

    def test_path_escape_is_rejected(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "manifest.json").write_text(json.dumps([{"filename": "../escape.rtf", "sha256": "x"}]), encoding="utf-8")
            with self.assertRaises(ValueError):
                inventory(root)

    def test_expectations_cannot_be_overwritten(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "expectations.json"
            write_json(path, {"reviewed": True}, exclusive=True)
            with self.assertRaises(FileExistsError):
                write_json(path, {"reviewed": False}, exclusive=True)
            self.assertTrue(json.loads(path.read_text(encoding="utf-8"))["reviewed"])


if __name__ == "__main__":
    unittest.main()
