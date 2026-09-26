"""Offline guard regression tests; no weights or ML dependencies required."""
from pathlib import Path
import subprocess
import sys
import unittest

ROOT = Path(__file__).resolve().parent


class ExportGuardTest(unittest.TestCase):
    def test_export_refuses_without_license_review_before_importing_torch(self):
        result = subprocess.run(
            [sys.executable, str(ROOT / 'export_gfpgan.py')],
            capture_output=True, text=True, check=False,
        )
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('BLOCKED: GFPGAN license review required', result.stderr)
        self.assertNotIn('ModuleNotFoundError', result.stderr)


if __name__ == '__main__':
    unittest.main()
