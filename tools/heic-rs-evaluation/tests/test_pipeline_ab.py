"""Fail-closed CLI validation tests for the pipeline A/B summarizer."""

import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest


SCRIPT = Path(__file__).resolve().parents[1] / "compare_pipeline_ab.py"


class PipelineAbFailClosedTests(unittest.TestCase):
    def run_invalid_receipt(self, mutate, expected_error):
        with tempfile.TemporaryDirectory() as temp_dir:
            root = Path(temp_dir)
            run_root = root / "runs"
            run_root.mkdir()
            runs = {
                "runs": [
                    {
                        "case": "synthetic",
                        "variant": "native",
                        "input": "synthetic.heic",
                        "input_sha256": "a" * 64,
                        "lane": "text",
                    },
                    {
                        "case": "synthetic",
                        "variant": "heic_rs",
                        "input": "synthetic.heic",
                        "input_sha256": "a" * 64,
                        "lane": "text",
                    },
                ]
            }
            mutate(runs["runs"])
            (run_root / "runs.json").write_text(
                json.dumps(runs), encoding="utf-8"
            )
            report = root / "comparison.json"

            completed = subprocess.run(
                [sys.executable, str(SCRIPT), str(run_root), "--output", str(report)],
                shell=False,
                timeout=10,
                capture_output=True,
                text=True,
                check=False,
            )

            self.assertNotEqual(completed.returncode, 0)
            self.assertFalse(report.exists(), "invalid receipt must not publish a report")
            self.assertIn(expected_error, completed.stderr + completed.stdout)

    def test_input_sha256_mismatch_is_rejected_before_results_are_read(self):
        self.run_invalid_receipt(
            lambda runs: runs[1].update(input_sha256="b" * 64),
            "synthetic/input_sha256",
        )

    def test_input_identifier_mismatch_is_rejected_before_results_are_read(self):
        self.run_invalid_receipt(
            lambda runs: runs[1].update(input="other.heic"),
            "synthetic/input",
        )

    def test_lane_mismatch_is_rejected_before_results_are_read(self):
        self.run_invalid_receipt(
            lambda runs: runs[1].update(lane="image"),
            "synthetic/lane",
        )

    def test_duplicate_variant_is_rejected_before_results_are_read(self):
        self.run_invalid_receipt(
            lambda runs: runs[1].update(variant="native"),
            "Duplicate or unknown A/B variant",
        )

    def test_unknown_variant_is_rejected_before_results_are_read(self):
        self.run_invalid_receipt(
            lambda runs: runs[1].update(variant="other"),
            "Duplicate or unknown A/B variant",
        )


if __name__ == "__main__":
    unittest.main()
