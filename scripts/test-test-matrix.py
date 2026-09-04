#!/usr/bin/env python3
"""Tests for evidence validation; does not launch Cargo or delete artifacts."""

import importlib.util
from pathlib import Path
import tempfile
import unittest

spec = importlib.util.spec_from_file_location("matrix", Path(__file__).with_name("test-matrix.py"))
matrix = importlib.util.module_from_spec(spec)
spec.loader.exec_module(matrix)


class MatrixEvidenceTests(unittest.TestCase):
    def test_configuration_set_is_complete(self):
        self.assertEqual(list(matrix.CONFIGURATIONS),
                         ["default", "no-default", "erased", "reified", "all-features"])

    def test_multiple_binaries_and_ignored_tests_remain_visible(self):
        totals = matrix.test_totals(
            "test result: ok. 7 passed; 0 failed; 2 ignored; 0 measured; 0 filtered out\n"
            "test result: ok. 4 passed; 0 failed; 1 ignored; 0 measured; 0 filtered out\n")
        self.assertEqual(totals, dict(passed=11, failed=0, ignored=3, measured=0, filtered=0))

    def test_empty_or_filtered_success_is_not_a_full_gate(self):
        for log in ("", "Finished test profile", "test result: FAILED.",
                    "test result: ok. 7 passed; 0 failed; 0 ignored; 0 measured; 2 filtered out"):
            with self.subTest(log=log), self.assertRaises(ValueError):
                matrix.test_totals(log)

    def test_resume_requires_success_validation_fingerprint_and_intact_log(self):
        with tempfile.TemporaryDirectory(prefix="rphp-matrix-selftest-") as directory:
            root = Path(directory)
            log = root / "default.log"
            log.write_text("successful original evidence\n")
            record = dict(exit=0, validated=True, fingerprint="current", log=log.name,
                          log_sha256=matrix.digest(log))
            self.assertTrue(matrix.reusable(record, root, "current"))
            for update in (dict(exit=1), dict(validated=False), dict(fingerprint="old")):
                self.assertFalse(matrix.reusable(dict(record, **update), root, "current"))
            log.write_text("changed evidence\n")
            self.assertFalse(matrix.reusable(record, root, "current"))
            log.unlink()
            self.assertFalse(matrix.reusable(record, root, "current"))


if __name__ == "__main__":
    unittest.main()
