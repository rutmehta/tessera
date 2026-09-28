"""Contract checks for the isolated visible P01 runner; no app or native engine is launched."""
import importlib.util
from pathlib import Path
import sys
import unittest

sys.dont_write_bytecode = True
SPEC = importlib.util.spec_from_file_location("visible_timing", Path(__file__).with_name("app_timing_visible.py"))
visible = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(visible)


def causal_trace(count=100):
    events = []
    for index in range(count):
        base = 10.0 + index
        common = {"session": "session-a", "input": index + 1, "generation": index + 1,
                  "level": 0, "residency": "resident"}
        events.extend([
            {"name": "input", "time": base, **common},
            {"name": "job_dequeue", "time": base + 0.1, **common},
            {"name": "callback_enqueue", "time": base + 0.2, **common},
            {"name": "drawable_presented", "time": base + 0.3, **common},
        ])
    return {"events": events, "dropped": 0}


class VisibleTimingTests(unittest.TestCase):
    def test_accepts_only_existing_causal_actual_present_oracle(self):
        summary = visible.validate_trace(causal_trace())
        self.assertEqual(summary["causally_presented_inputs"], 100)
        self.assertTrue(summary["p01_complete"])

    def test_callback_without_positive_actual_present_is_rejected(self):
        trace = causal_trace()
        trace["events"] = [event for event in trace["events"]
                           if event["name"] != "drawable_presented"]
        with self.assertRaisesRegex(ValueError, "only 0 inputs"):
            visible.validate_trace(trace)
        trace = causal_trace()
        trace["events"][3]["time"] = 0
        with self.assertRaisesRegex(ValueError, "non-positive actual timestamp"):
            visible.validate_trace(trace)

    def test_trace_drops_are_not_a_presentation_pass(self):
        trace = causal_trace()
        trace["dropped"] = 1
        with self.assertRaisesRegex(ValueError, "dropped"):
            visible.validate_trace(trace)

    def test_source_pinned_unique_release_identity_is_required(self):
        visible.validate_bundle_identity("dev.tessera.m258.visible.abc123", "dev.tessera.m258.visible.abc123")
        visible.validate_provenance({"commit": "abc123", "configuration": "release"}, "abc123")
        with self.assertRaisesRegex(ValueError, "unique"):
            visible.validate_bundle_identity("dev.tessera.app", "dev.tessera.app")
        with self.assertRaisesRegex(ValueError, "source commit"):
            visible.validate_provenance({"commit": "different", "configuration": "release"}, "abc123")
        with self.assertRaisesRegex(ValueError, "Release"):
            visible.validate_provenance({"commit": "abc123", "configuration": "debug"}, "abc123")

    def test_foreground_window_requires_no_reported_foreign_occluder(self):
        target = {"owner_pid": 42, "window_id": 7, "layer": 0, "onscreen": True,
                  "bounds": {"x": 0, "y": 0, "width": 100, "height": 100}, "alpha": 1}
        visible.validate_window_observation({"frontmost_pid": 42, "windows": [target]}, 42, 7)
        foreign = {"owner_pid": 99, "window_id": 8, "layer": 0, "onscreen": True,
                   "bounds": {"x": 20, "y": 20, "width": 10, "height": 10}, "alpha": 1}
        with self.assertRaisesRegex(ValueError, "overlaps"):
            visible.validate_window_observation({"frontmost_pid": 42, "windows": [foreign, target]}, 42, 7)
        with self.assertRaisesRegex(ValueError, "foreground"):
            visible.validate_window_observation({"frontmost_pid": 99, "windows": [target]}, 42, 7)

    def test_window_startup_allowance_is_explicit_and_focus_loss_fails(self):
        startup, visible_window, _ = visible.validate_visibility_sample(
            {"frontmost_pid": 1, "windows": []}, 42, 2.0, False, False, startup_allowance=5.0)
        self.assertFalse(startup)
        self.assertFalse(visible_window)
        with self.assertRaisesRegex(ValueError, "startup allowance"):
            visible.validate_visibility_sample(
                {"frontmost_pid": 1, "windows": []}, 42, 5.1, False, False, startup_allowance=5.0)
        target = {"owner_pid": 42, "window_id": 7, "layer": 0, "onscreen": True,
                  "bounds": {"x": 0, "y": 0, "width": 100, "height": 100}, "alpha": 1}
        startup, visible_window, _ = visible.validate_visibility_sample(
            {"frontmost_pid": 42, "windows": [target]}, 42, 0.1, False, False)
        self.assertTrue(startup)
        self.assertTrue(visible_window)
        with self.assertRaisesRegex(ValueError, "lost foreground"):
            visible.validate_visibility_sample(
                {"frontmost_pid": 1, "windows": [target]}, 42, 0.2, True, True)


if __name__ == "__main__":
    unittest.main()
