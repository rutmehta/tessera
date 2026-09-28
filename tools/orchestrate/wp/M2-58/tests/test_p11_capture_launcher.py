import importlib.util
import json
import os
import pathlib
import tempfile
import time
import unittest


ROOT = pathlib.Path(__file__).resolve().parents[1]
SPEC = importlib.util.spec_from_file_location(
    "p11_capture_launcher", ROOT / "p11_capture_launcher.py"
)
launcher = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(launcher)


class CaptureLauncherTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = pathlib.Path(self.temp.name) / "betterSSD"
        self.root.mkdir()
        self.app = self.root / "TesseraM258Test.app"
        contents = self.app / "Contents"
        contents.mkdir(parents=True)
        (contents / "Info.plist").write_bytes(
            b"<?xml version='1.0' encoding='UTF-8'?>"
            b"<!DOCTYPE plist PUBLIC '-//Apple//DTD PLIST 1.0//EN' "
            b"'http://www.apple.com/DTDs/PropertyList-1.0.dtd'>"
            b"<plist version='1.0'><dict><key>CFBundleIdentifier</key>"
            b"<string>dev.tessera.m258.test</string></dict></plist>"
        )
        self.helper = self.root / "helper"
        self.output = self.root / "run-output"
        self.config = self.root / "config.json"
        self.record = self.root / "launcher-record.json"
        self.payload = {
            "output_dir": str(self.output),
            "bundle_id": "dev.tessera.m258.test",
            "app_path": str(self.app),
            "pid": 67652,
            "launch_epoch": 1790611200.0,
            "window_id": 123,
            "display_id": 1,
            "roi_display_points": {"x": 10, "y": 20, "width": 320, "height": 240},
        }

    def write_helper(self, source):
        self.helper.write_text("#!/usr/bin/env python3\n" + source)
        self.helper.chmod(0o755)

    def write_config(self, payload=None):
        self.config.write_text(json.dumps(payload or self.payload))

    def test_records_direct_exit_and_exact_input_hashes(self):
        self.write_helper("import sys\nprint('stand-in complete')\nsys.exit(7)\n")
        self.write_config()

        result = launcher.execute(
            self.helper, self.config, self.record, allowed_root=self.root, timeout=3
        )

        self.assertEqual(result, 7)
        record = json.loads(self.record.read_text())
        self.assertEqual(record["outcome"], "exited")
        self.assertEqual(record["direct_exit"], 7)
        self.assertEqual(record["launcher_exit"], 7)
        self.assertEqual(record["config_sha256"], launcher.sha256_file(self.config))
        self.assertEqual(record["config_snapshot_sha256"], launcher.sha256_file(f"{self.record}.config.json"))
        self.assertEqual(record["config_snapshot_sha256_after"], record["config_snapshot_sha256"])
        saved_config = json.loads(pathlib.Path(record["configuration_snapshot"]).read_text())
        self.assertEqual(saved_config["app_path"], str(self.app.resolve()))
        self.assertEqual(saved_config["output_dir"], str(self.output.resolve()))
        self.assertEqual(record["helper_sha256"], launcher.sha256_file(self.helper))
        self.assertEqual(record["scope"]["pid"], 67652)
        self.assertEqual(record["scope"]["window_id"], 123)
        self.assertEqual(record["scope"]["display_id"], 1)
        self.assertEqual(record["scope"]["roi_display_points"], self.payload["roi_display_points"])
        self.assertFalse(record["shell"])

    def test_mutated_passed_config_fails_integrity_without_hiding_direct_exit(self):
        self.write_helper(
            "import pathlib, sys\n"
            "pathlib.Path(sys.argv[1]).write_text('{}')\n"
            "raise SystemExit(0)\n"
        )
        self.write_config()

        result = launcher.execute(
            self.helper, self.config, self.record, allowed_root=self.root, timeout=3
        )

        self.assertEqual(result, 126)
        record = json.loads(self.record.read_text())
        self.assertEqual(record["outcome"], "exited")
        self.assertEqual(record["direct_exit"], 0)
        self.assertEqual(record["launcher_exit"], 126)
        self.assertIn("snapshot changed", record["integrity_error"])

    def test_invalid_roi_fails_closed_before_starting_helper(self):
        marker = self.root / "started"
        self.write_helper(f"import pathlib\npathlib.Path({str(marker)!r}).touch()\n")
        payload = dict(self.payload, roi_display_points={"x": 0, "y": 0, "width": 513, "height": 10})
        self.write_config(payload)

        result = launcher.execute(
            self.helper, self.config, self.record, allowed_root=self.root, timeout=3
        )

        self.assertEqual(result, 2)
        self.assertFalse(marker.exists())
        record = json.loads(self.record.read_text())
        self.assertEqual(record["outcome"], "preflight_error")
        self.assertIsNone(record["direct_exit"])
        self.assertEqual(record["launcher_exit"], 2)
        self.assertIn("ROI", record["error"])

    def test_existing_receipt_is_never_overwritten_or_executed(self):
        marker = self.root / "started"
        self.write_helper(f"import pathlib\npathlib.Path({str(marker)!r}).touch()\n")
        self.write_config()
        self.record.write_text("original evidence\n")

        with self.assertRaises(FileExistsError):
            launcher.execute(self.helper, self.config, self.record, allowed_root=self.root, timeout=3)

        self.assertEqual(self.record.read_text(), "original evidence\n")
        self.assertFalse(marker.exists())

    def test_deadline_covers_helper_startup_and_kills_only_owned_child(self):
        marker = self.root / "child.pid"
        self.write_helper(
            "import os, pathlib, time\n"
            f"pathlib.Path({str(marker)!r}).write_text(str(os.getpid()))\n"
            "time.sleep(0.5)  # stand in for slow initial config/startup work\n"
            "import json, sys\n"
            "cfg = json.loads(pathlib.Path(sys.argv[1]).read_text())\n"
            "pathlib.Path(cfg['output_dir']).mkdir()\n"
        )
        self.write_config()

        started = time.monotonic()
        result = launcher.execute(
            self.helper, self.config, self.record, allowed_root=self.root, timeout=0.3
        )
        elapsed = time.monotonic() - started

        self.assertEqual(result, 124)
        self.assertLess(elapsed, 5)
        record = json.loads(self.record.read_text())
        self.assertEqual(record["outcome"], "timeout")
        self.assertIsNone(record["direct_exit"])
        self.assertEqual(record["launcher_exit"], 124)
        child_pid = int(marker.read_text())
        self.assertEqual(record["helper_pid"], child_pid)
        self.assertEqual(record["timeout_action"], "killed_owned_helper_process_only")
        with self.assertRaises(ProcessLookupError):
            os.kill(child_pid, 0)
        self.assertFalse(self.output.exists(), "timeout must cover helper startup before output-directory creation")

    def test_preexisting_output_fails_closed_without_starting_helper(self):
        marker = self.root / "started"
        self.write_helper(f"import pathlib\npathlib.Path({str(marker)!r}).touch()\n")
        self.output.mkdir()
        self.write_config()

        result = launcher.execute(
            self.helper, self.config, self.record, allowed_root=self.root, timeout=3
        )

        self.assertNotEqual(result, 0)
        self.assertFalse(marker.exists())
        record = json.loads(self.record.read_text())
        self.assertEqual(record["outcome"], "preflight_error")
        self.assertIn("must not already exist", record["error"])

    def test_app_bundle_identifier_mismatch_fails_closed_before_helper(self):
        marker = self.root / "started"
        self.write_helper(f"import pathlib\npathlib.Path({str(marker)!r}).touch()\n")
        payload = dict(self.payload, bundle_id="dev.tessera.m258.wrong")
        self.write_config(payload)

        result = launcher.execute(
            self.helper, self.config, self.record, allowed_root=self.root, timeout=3
        )

        self.assertNotEqual(result, 0)
        self.assertFalse(marker.exists())
        record = json.loads(self.record.read_text())
        self.assertEqual(record["outcome"], "preflight_error")
        self.assertIsNone(record["direct_exit"])
        self.assertEqual(record["launcher_exit"], 2)
        self.assertIn("does not match", record["error"])


if __name__ == "__main__":
    unittest.main()
