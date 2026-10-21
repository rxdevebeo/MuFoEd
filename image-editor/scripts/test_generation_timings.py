"""Regression checks for cross-thread completion order and wall-clock quantization."""
import json
from pathlib import Path
import tempfile
import unittest
from summarize_generation_bench import summarize


class TimingTests(unittest.TestCase):
    def events(self):
        def event(name, start, duration):
            return {"stage": name, "start_ms": start, "elapsed_ms": duration,
                    "source": "rust", "sync_error": None}
        return [event("unet.step", 10, 20),
                event("native.TextToImage.1024x1024", 0, 30),
                event("pipeline.first", 0, 40),
                event("pipeline.ultrasharp", 40, 10),
                event("unet.step", 51, 20),
                event("native.ImageToImage.1536x1536", 50, 40),
                event("pipeline.refine", 52, 51),  # Ends after parent according to epoch clock.
                event("pipeline.total", 0, 100)]

    def summarize(self, events):
        with tempfile.TemporaryDirectory() as folder:
            profile = Path(folder) / "timings.jsonl"
            profile.write_text("\n".join(json.dumps(e) for e in events), encoding="utf-8")
            return summarize(profile, {"runs": [{"run": "cold", "elapsed_s": 0.101}]})

    def test_nested_timings_do_not_double_count_or_require_epoch_containment(self):
        result = self.summarize(self.events())
        self.assertAlmostEqual(result["runs"][0]["wall_s"], 0.101)
        self.assertAlmostEqual(result["runs"][0]["pipeline.first"], 0.04)
        self.assertEqual([v["stages"]["unet.step"]["count"] for v in result["native_details"]], [1, 1])

    def test_partial_logs_and_failed_gpu_sync_are_rejected(self):
        with self.assertRaises(ValueError):
            self.summarize(self.events()[:-1])
        events = self.events()
        events[0]["sync_error"] = "device error"
        with self.assertRaises(ValueError):
            self.summarize(events)


if __name__ == "__main__":
    unittest.main()
