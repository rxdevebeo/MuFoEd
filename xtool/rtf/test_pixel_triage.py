"""Independent assertions on synthetic receipts, not on the real corpus.

Each test builds the smallest receipt pair that exercises one gate mechanism,
so the classifier's contract is pinned without depending on 97 real documents.
"""

import unittest

from pixel_triage import classify, page_kind

FAIL_OK = (
    'Fail\tssim=0.9\tthreshold=0.96\tstructure=Ok(StructuralFidelity { shift_y_px: 0 })'
)
CANDIDATE_LOW = (
    'Fail\tssim=0.9\tthreshold=0.96\tstructure=Err("candidate ink 0.00000 << reference 0.00829")'
)
CANDIDATE_HIGH = (
    'Fail\tssim=0.9\tthreshold=0.96\tstructure=Err("candidate ink 0.00966 >> reference 0.00336")'
)
REFERENCE_EMPTY = (
    'Fail\tssim=1.0\tthreshold=0.96\tstructure=Err("reference ink 0.00000 is empty")'
)
VERTICAL = (
    'Fail\tssim=0.6\tthreshold=0.96\tstructure=Err("vertical shift -4px exceeds tolerance [measured: shift (0px,-4px)]")'
)
ROW = (
    'Fail\tssim=0.6\tthreshold=0.96\tstructure=Err("row-ink correlation 0.443 too low [measured: shift (0px,-7px)]")'
)


def receipt(stem, pages, pixels, visual_pages=1, actual=1):
    pixel_row = {"filename": stem + ".rtf", "pixels": pixels}
    if pages is not None:
        pixel_row["pages"] = [
            {"page": i + 1, "verdict": "Pass" if kind is None else "Fail",
             "measurement": "Pass\tssim=1.0\tthreshold=0.96\tstructure=Ok(x)" if kind is None else kind}
            for i, kind in enumerate(pages)
        ]
    visual_row = {"filename": stem + ".rtf", "pagination": "Fail" if pages is None else "Pass",
                  "render_status": "Clean", "actual_pages": actual, "reference_pages": visual_pages}
    return pixel_row, visual_row


def model(specs):
    pixels = {"dpi": 96, "pagination_receipt_sha256": "x", "comparison_binary_sha256": "y",
              "files": [spec[0] for spec in specs]}
    visual = {"files": [spec[1] for spec in specs]}
    return classify(pixels, visual)


class PageKindTests(unittest.TestCase):
    def test_first_violated_bound_names_the_mechanism(self):
        self.assertEqual(page_kind(CANDIDATE_LOW), "missing_ink")
        self.assertEqual(page_kind(CANDIDATE_HIGH), "extra_ink")
        self.assertEqual(page_kind(REFERENCE_EMPTY), "blank_reference")
        self.assertEqual(page_kind(VERTICAL), "vertical_shift")
        self.assertEqual(page_kind(ROW), "row_correlation")
        self.assertEqual(page_kind(FAIL_OK), "glyph_appearance")

    def test_unknown_measurement_is_not_silently_classified(self):
        with self.assertRaises(ValueError):
            page_kind('Fail\tssim=0.1\tthreshold=0.96\tstructure=Err("mystery bound")')


class ClassifyTests(unittest.TestCase):
    def test_pass_and_pagination_are_disjoint_top_classes(self):
        triage = model([
            receipt("040", None, "Pass"),
            receipt("001", None, "Fail", visual_pages=9, actual=11),
        ])
        by_name = {entry["filename"]: entry for entry in triage["files"]}
        self.assertEqual(by_name["040.rtf"]["primary"], "pass_control")
        self.assertEqual(by_name["001.rtf"]["primary"], "pagination_page_count")
        self.assertEqual(by_name["001.rtf"]["page_count_delta"], 2)
        self.assertEqual(triage["evidence"]["pagination_ours_more"], 1)
        self.assertEqual(triage["evidence"]["pagination_ours_fewer"], 0)

    def test_specific_mechanisms_are_document_primary(self):
        triage = model([
            receipt("035", [CANDIDATE_LOW], "Fail"),
            receipt("036", [REFERENCE_EMPTY], "Fail"),
            receipt("037", [FAIL_OK], "Fail"),
        ])
        primary = {entry["filename"]: entry["primary"] for entry in triage["files"]}
        self.assertEqual(primary["035.rtf"], "missing_ink")
        self.assertEqual(primary["036.rtf"], "blank_reference")
        self.assertEqual(primary["037.rtf"], "glyph_appearance")

    def test_dominant_mechanism_wins_and_the_rest_are_secondary(self):
        triage = model([receipt("009", [REFERENCE_EMPTY, ROW, ROW], "Fail")])
        entry = triage["files"][0]
        self.assertEqual(entry["primary"], "line_metrics")
        self.assertEqual(entry["secondary"], ["blank_reference"])
        self.assertEqual(entry["page_kinds"], {"blank_reference": 1, "row_correlation": 2})

    def test_page_mechanisms_and_documents_are_exact_and_disjoint(self):
        triage = model([
            receipt("009", [REFERENCE_EMPTY, ROW, ROW], "Fail"),
            receipt("037", [FAIL_OK], "Fail"),
            receipt("040", None, "Pass"),
        ])
        self.assertEqual(triage["summary"]["documents"], 3)
        self.assertEqual(triage["summary"]["pixel_pass"], 1)
        self.assertEqual(triage["summary"]["pixel_fail"], 2)
        self.assertEqual(triage["summary"]["pagination_page_count"], 0)
        self.assertEqual(triage["summary"]["matched_page_pixel"], 2)
        self.assertEqual(triage["summary"]["measured_pages"], 4)
        self.assertEqual(triage["summary"]["failing_pages"], 4)
        self.assertEqual(triage["page_mechanisms"], {"line_metrics": 2, "blank_reference": 1, "glyph_appearance": 1})
        documents = sum(len(group["documents"]) for group in triage["classes"].values())
        self.assertEqual(documents, 3)

    def test_match_control_pages_count_as_passing(self):
        triage = model([receipt("025", [None, ROW], "Fail")])
        entry = triage["files"][0]
        self.assertEqual(entry["measured_pages"], 2)
        self.assertEqual(entry["pass_pages"], 1)
        self.assertEqual(triage["summary"]["passing_pages"], 1)


if __name__ == "__main__":
    unittest.main()
