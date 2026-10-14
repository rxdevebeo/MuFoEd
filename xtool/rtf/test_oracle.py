"""Test oracle bookkeeping without claiming an available office engine."""

from pathlib import Path
import tempfile
import unittest

from oracle import capture, executable_path
import test_census


class OracleTests(unittest.TestCase):
    def test_missing_engine_blocks_all_hundred_inputs(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            test_census.InventoryTests().make_corpus(root)
            report, exit_code = capture(root, root / "output", None, 1)
            self.assertEqual(exit_code, 2)
            self.assertEqual(len(report["files"]), 100)
            self.assertTrue(all(row["status"] == "Blocked" for row in report["files"]))
            self.assertIsNone(report["oracle"])

    def test_explicit_missing_engine_is_not_silently_replaced(self):
        with tempfile.TemporaryDirectory() as directory:
            self.assertIsNone(executable_path(str(Path(directory) / "absent.exe")))


if __name__ == "__main__":
    unittest.main()
