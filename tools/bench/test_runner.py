import copy
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
import runner


class SchemaTests(unittest.TestCase):
    def test_schema_rejects_invalid_values_and_duplicates(self):
        row = dict(bench="index-search", fixture="100k-v1", metric="median_ms",
                   value=10.0, unit="ms", backend="cpu", host="test-host")
        self.assertEqual(runner.validate([row]), [row])
        for value in [-1, 0, float("nan"), float("inf"), True, "10"]:
            with self.subTest(value=value), self.assertRaises(ValueError):
                runner.validate([{**row, "value": value}])
        for field in row:
            bad = copy.copy(row)
            del bad[field]
            with self.subTest(field=field), self.assertRaises(ValueError):
                runner.validate([bad])
        with self.assertRaises(ValueError):
            runner.validate([row, row])
        with self.assertRaises(ValueError):
            runner.validate([])

    def test_schema_document_and_checked_in_baselines(self):
        schema = json.loads((Path(runner.__file__).parent / "results.schema.json").read_text())
        self.assertEqual(set(schema["items"]["required"]), runner.FIELDS)
        baselines = list(Path(runner.__file__).parent.glob("bench-baseline*.json"))
        self.assertTrue(baselines, "measured host baselines must be present")
        for path in baselines:
            baseline = json.loads(path.read_text())
            runner.validate(baseline["results"])
            self.assertFalse(runner.compare(baseline["results"], baseline)[1])

    def test_identity_fields_and_enums_are_validated(self):
        row = dict(bench="index-search", fixture="100k-v1", metric="median_ms",
                   value=10, unit="ms", backend="cpu", host="test-host")
        for field, value in [("host", "../outside"), ("unit", "seconds"),
                             ("backend", "auto"), ("bench", " "), ("metric", 42)]:
            with self.subTest(field=field), self.assertRaises(ValueError):
                runner.validate([{**row, field: value}])
        with self.assertRaises(ValueError):
            runner.validate([{**row, "unexpected": 1}])


class ComparisonTests(unittest.TestCase):
    def setUp(self):
        self.row = dict(bench="index-search", fixture="100k-v1", metric="median_ms",
                        value=100.0, unit="ms", backend="cpu", host="test-host")
        self.baseline = {"schema_version": 1, "results": [self.row],
                         "tolerances": {"median_ms": 0.15}}

    def test_boundary_regression_and_improvement(self):
        for value, failed in [(115, False), (115.01, True), (90, False)]:
            self.assertEqual(runner.compare([{**self.row, "value": value}],
                                            self.baseline)[1], failed)

    def test_per_bench_tolerance_overrides_metric_default(self):
        baseline = {**self.baseline, "tolerances": {
            "median_ms": .15, "index-search/median_ms": .25}}
        self.assertFalse(runner.compare([{**self.row, "value": 120}], baseline)[1])
        self.assertTrue(runner.compare([{**self.row, "value": 126}], baseline)[1])

    def test_order_is_not_identity_and_missing_rows_fail(self):
        second = {**self.row, "bench": "second"}
        baseline = {**self.baseline, "results": [self.row, second]}
        self.assertFalse(runner.compare([second, self.row], baseline)[1])
        with self.assertRaises(ValueError):
            runner.compare([self.row], baseline)

    def test_missing_mismatched_and_invalid_baselines_fail_closed(self):
        for field, value in [("host", "other"), ("fixture", "new"),
                             ("metric", "p90_ms"), ("backend", "gpu")]:
            with self.subTest(field=field), self.assertRaises(ValueError):
                runner.compare([{**self.row, field: value}], self.baseline)
        for tolerance in [-1, float("nan"), True, "15%"]:
            bad = {**self.baseline, "tolerances": {"median_ms": tolerance}}
            with self.assertRaises(ValueError):
                runner.compare([self.row], bad)
        with self.assertRaises(ValueError):
            runner.compare([self.row], {**self.baseline, "tolerances": {}})
        with self.assertRaises(ValueError):
            runner.compare([self.row], {**self.baseline, "schema_version": 2})


class AdapterTests(unittest.TestCase):
    def test_gpu_parsers_require_real_measurements(self):
        develop = "Metal (Apple M4) panels, screen L2 1000×800:\n  tone exposure     L2: median 1.1 ms, p90 1.5 ms\n"
        rows = runner.parse_gpu("develop", develop, "test-host")
        self.assertEqual([r["value"] for r in rows], [1.1, 1.5])
        self.assertIn("L2", rows[0]["fixture"])
        export = "BENCH file=sony-arw.ARW preset=web requested=gpu used_gpu=true lens_off=false seconds=0.123"
        self.assertEqual(runner.parse_gpu("export", export, "test-host")[0]["value"], 123)
        resident = "L2 full recomposite (100 layers, 1368×912): min 3.00 ms, median 4.00 ms\n64² dab → L2 recomposite: min 0.10 ms, median 0.20 ms, max 0.30 ms\nL0 full composite (20 MP × 100 layers): min 10.0 ms, median 11.0 ms"
        self.assertEqual([r["value"] for r in runner.parse_gpu("resident", resident, "test-host")], [4, .2, 11])
        for name, text in [("develop", "skipping: no fixture"),
                           ("develop", develop.replace("Metal (Apple M4)", "CPU ×4")),
                           ("export", export.replace("used_gpu=true", "used_gpu=false")),
                           ("resident", resident.splitlines()[0]), ("resident", "")]:
            with self.subTest(name=name), self.assertRaises(ValueError):
                runner.parse_gpu(name, text, "test-host")


class CliTests(unittest.TestCase):
    def test_subprocess_failures_and_timeouts_are_errors(self):
        with tempfile.TemporaryDirectory() as tmp:
            log = Path(tmp) / "worker.log"
            with self.assertRaisesRegex(ValueError, "command failed"):
                runner.command([sys.executable, "-c", "raise SystemExit(7)"],
                               os.environ.copy(), log)
            with self.assertRaisesRegex(ValueError, "timed out"):
                runner.command([sys.executable, "-c", "import time; time.sleep(10)"],
                               os.environ.copy(), log, timeout=.05)

    def test_record_replay_regression_missing_and_gate(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            rows = [dict(bench=b, fixture=f, metric="median_ms", value=100,
                         backend="cpu", unit="ms", host="test-host")
                    for b, f in runner.CPU_FIXTURES.items()]
            source = root / "input.json"
            baseline = root / "baseline.json"
            source.write_text(json.dumps(rows))
            command = [sys.executable, "-B", str(Path(runner.__file__)),
                       "--input", str(source), "--host", "test-host",
                       "--baseline", str(baseline), "--output-dir", str(root / "results")]

            def run(gate, *args):
                return subprocess.run(command + list(args), capture_output=True, text=True,
                                      env={**os.environ, "BENCH_GATE": str(gate)})

            self.assertEqual(run(1).returncode, 2)  # no baseline
            self.assertEqual(run(1, "--record-baseline").returncode, 2)
            result = run(0, "--record-baseline")
            self.assertEqual(result.returncode, 0, result.stderr)
            original = baseline.read_bytes()
            self.assertEqual(run(1).returncode, 0)
            rows[0]["value"] = 116
            source.write_text(json.dumps(rows))
            self.assertEqual(run(1).returncode, 1)
            self.assertEqual(run(0).returncode, 0)
            self.assertEqual(baseline.read_bytes(), original)
            self.assertTrue(list((root / "results" / "test-host").glob("*.json")))
            source.write_text(json.dumps(rows[:-1]))
            self.assertEqual(run(0, "--record-baseline").returncode, 2)
            self.assertEqual(baseline.read_bytes(), original)


if __name__ == "__main__":
    unittest.main()
