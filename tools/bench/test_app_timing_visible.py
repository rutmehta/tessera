"""Contract checks for the isolated visible P01 runner; no app or native engine is launched."""
import importlib.util
from pathlib import Path
import sys
import tempfile
import unittest

sys.dont_write_bytecode = True
SPEC = importlib.util.spec_from_file_location("visible_timing", Path(__file__).with_name("app_timing_visible.py"))
visible = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(visible)


def causal_trace(count=100):
    events = []
    for index in range(count):
        base = 10.0 + index
        common = {"session": "session-a", "input": index + 1,
                  "generation": index + 1, "level": 0, "residency": "resident"}
        events.extend([
            {"name": "app_visibility_check", "time": base - .01, "session": "session-a",
             "span": f"scripted-input-{index + 1}"},
            {"name": "input", "time": base, "session": "session-a", "input": index + 1},
            {"name": "job_dequeue", "time": base + 0.1, **common},
            {"name": "callback_enqueue", "time": base + 0.2, **common},
            {"name": "drawable_presented", "time": base + 0.3, **common},
        ])
    return {"events": events, "dropped": 0}


def visibility_samples(start, end, pid=42, window=7):
    first = start - .05
    samples = []
    current = first
    while current <= end + .05:
        samples.append({"time": round(current, 6), "pid": pid, "bundle_id": "dev.tessera.test",
                        "bundle_url": "/tmp/Test.app", "launch_date": 1_800_000_000.0,
                        "window_number": window, "session": "session-a", "nonce": "n-1",
                        "frontmost": True, "visible": True})
        current += .05
    samples.append({"time": round(end + .05, 6), "pid": pid, "bundle_id": "dev.tessera.test",
                    "bundle_url": "/tmp/Test.app", "launch_date": 1_800_000_000.0,
                    "window_number": window, "session": "session-a", "nonce": "n-1",
                    "frontmost": True, "visible": True})
    return samples


def expected_identity():
    return {"pid": 42, "bundle_id": "dev.tessera.test", "bundle_url": "/tmp/Test.app",
            "launch_date": 1_800_000_000.0, "window_number": 7, "session": "session-a"}


def ready_record(time=8.0):
    return {"nonce": "n-1", **expected_identity(), "time": time}


class VisibleTimingTests(unittest.TestCase):
    def test_stable_foreground_dwell_uses_dense_monotonic_probe_times(self):
        dwell = visible.StableForegroundDwell()
        self.assertFalse(dwell.observe(1.0))
        self.assertFalse(dwell.observe(1.25))
        self.assertTrue(dwell.observe(1.5))
        reset = visible.StableForegroundDwell()
        self.assertFalse(reset.observe(2.0))
        self.assertFalse(reset.observe(2.251))
        self.assertFalse(reset.observe(2.5))

    def test_qualified_interval_counts_only_complete_chains_inside_one_bound_identity(self):
        trace = causal_trace(121)
        trace["events"].extend([
            {"name": "measurement_start", "time": 9.0, "session": "session-a"},
            {"name": "input_sequence_complete", "time": 131.4, "session": "session-a", "input": 121},
            {"name": "measurement_end", "time": 132.0, "session": "session-a"},
        ])
        ready = ready_record()
        permit = {**ready, "time": 8.8}
        samples = visibility_samples(9.0, 132.0)
        result = visible.validate_qualified_interval(
            trace, ready, permit, samples,
            expected_identity(), "n-1")
        self.assertEqual(result["causal_presentations"], 121)
        self.assertAlmostEqual(result["input_to_present_ms"][0], 300.0)
        self.assertEqual(result["final_input"], 121)

    def test_qualified_interval_rejects_insufficient_or_incomplete_final_drain(self):
        trace = causal_trace(121)
        trace["events"].extend([
            {"name": "measurement_start", "time": 9.0, "session": "session-a"},
            {"name": "input_sequence_complete", "time": 131.0, "session": "session-a", "input": 121},
            {"name": "measurement_end", "time": 131.2, "session": "session-a"},
        ])
        ready = ready_record()
        samples = visibility_samples(9.0, 131.2)
        events = trace["events"]
        trace["events"] = [e for e in events if not (e["name"] == "job_dequeue" and e.get("input") in range(1, 23))]
        with self.assertRaisesRegex(ValueError, "fewer than 100"):
            visible.validate_qualified_interval(trace, ready, {**ready, "time": 8.8}, samples,
                expected_identity(), "n-1")
        trace["events"] = events
        trace["events"] = [e for e in events if not (e["name"] == "drawable_presented" and e.get("input") == 121)]
        with self.assertRaisesRegex(ValueError, "final input"):
            visible.validate_qualified_interval(trace, ready, {**ready, "time": 8.8}, samples,
                expected_identity(), "n-1")

        trace["events"] = [e for e in events if not (e["name"] == "drawable_presented" and e.get("input") == 121)] + [{"name": "drawable_presented", "time": 131.3,
            "session": "session-a", "input": 121, "generation": 121, "level": 0, "residency": "resident"}]
        with self.assertRaisesRegex(ValueError, "final input"):
            visible.validate_qualified_interval(trace, ready, {**ready, "time": 8.8}, samples,
                expected_identity(), "n-1")

    def test_qualified_join_uses_real_input_schema_and_handles_frame_refinement(self):
        trace = causal_trace(121)
        frame = {"session": "session-a", "input": 1, "generation": 122, "level": 1,
                 "residency": "resident"}
        trace["events"] = [event for event in trace["events"]
                           if not (event.get("name") in ("job_dequeue", "callback_enqueue", "drawable_presented")
                                   and event.get("input") in range(1, 23))]
        trace["events"].extend([
            {"name": "job_dequeue", "time": 10.11, **frame},
            {"name": "callback_enqueue", "time": 10.12, **frame},
            {"name": "drawable_presented", "time": 10.13, **frame},
            {"name": "drawable_presented", "time": 10.14, **frame},
            {"name": "measurement_start", "time": 9.0, "session": "session-a"},
            {"name": "input_sequence_complete", "time": 131.0, "session": "session-a", "input": 121},
            {"name": "measurement_end", "time": 131.2, "session": "session-a"},
        ])
        ready = ready_record()
        result = visible.validate_qualified_interval(trace, ready, {**ready, "time": 8.8},
            visibility_samples(9.0, 131.2),
            expected_identity(), "n-1")
        self.assertEqual(result["causal_presentations"], 100)
        self.assertAlmostEqual(result["input_to_present_ms"][0], 130.0)
        wrong_input = dict(trace)
        wrong_input["events"] = [dict(event) for event in trace["events"]]
        for event in wrong_input["events"]:
            if event.get("name") == "drawable_presented" and event.get("generation") == 122:
                event["input"] = 999
        self.assertEqual(result["causal_presentations"], 100)
        with self.assertRaisesRegex(ValueError, "fewer than 100"):
            visible.validate_qualified_interval(wrong_input, ready, {**ready, "time": 8.8},
                visibility_samples(9.0, 131.2),
                expected_identity(), "n-1")

    def test_qualified_metrics_ignore_startup_and_other_sessions(self):
        trace = causal_trace(121)
        for index in range(1000, 1021):
            trace["events"].extend([
                {"name": "input", "time": 2.0, "session": "session-a", "input": index},
                {"name": "job_dequeue", "time": 2.1, "session": "session-a", "input": index,
                 "generation": index, "level": 0},
                {"name": "callback_enqueue", "time": 2.2, "session": "session-a", "input": index,
                 "generation": index, "level": 0, "residency": "resident"},
                {"name": "drawable_presented", "time": 120.0, "session": "session-a", "input": index,
                 "generation": index, "level": 0},
            ])
        trace["events"].extend([
            {"name": "input", "time": 2.0, "session": "other-session", "input": 1},
            {"name": "job_dequeue", "time": 2.1, "session": "other-session", "input": 1,
             "generation": 1, "level": 0},
            {"name": "callback_enqueue", "time": 2.2, "session": "other-session", "input": 1,
             "generation": 1, "level": 0, "residency": "resident"},
            {"name": "drawable_presented", "time": 120.0, "session": "other-session", "input": 1,
             "generation": 1, "level": 0},
            {"name": "measurement_start", "time": 9.0, "session": "session-a"},
            {"name": "input_sequence_complete", "time": 131.0, "session": "session-a", "input": 121},
            {"name": "measurement_end", "time": 131.2, "session": "session-a"},
        ])
        ready = ready_record()
        result = visible.validate_qualified_interval(trace, ready, {**ready, "time": 8.8},
            visibility_samples(9.0, 131.2),
            expected_identity(), "n-1")
        self.assertEqual(result["causal_presentations"], 121)
        self.assertEqual(len(result["input_to_present_ms"]), 121)
        self.assertLess(result["input_to_present_p95_ms"], 1000)
        self.assertGreater(visible.app_timing.summarize(trace)["input_to_present_p95_ms"], 1000)

    def test_qualified_interval_rejects_duplicate_markers_and_dropped_records(self):
        trace = causal_trace(121)
        trace["events"].extend([
            {"name": "measurement_start", "time": 9.0, "session": "session-a"},
            {"name": "input_sequence_complete", "time": 131.0, "session": "session-a", "input": 121},
            {"name": "measurement_end", "time": 131.2, "session": "session-a"},
        ])
        ready = ready_record()
        args = (trace, ready, {**ready, "time": 8.8}, visibility_samples(9.0, 131.2),
                expected_identity(), "n-1")
        trace["events"].append({"name": "measurement_start", "time": 9.1, "session": "session-a"})
        with self.assertRaisesRegex(ValueError, "exactly one"):
            visible.validate_qualified_interval(*args)
        trace["events"].pop()
        trace["dropped"] = 1
        with self.assertRaisesRegex(ValueError, "dropped"):
            visible.validate_qualified_interval(*args)

    def test_qualified_interval_rejects_duplicate_input_and_nonfinite_causal_timestamp(self):
        trace = causal_trace(121)
        trace["events"].extend([
            {"name": "measurement_start", "time": 9.0, "session": "session-a"},
            {"name": "input_sequence_complete", "time": 131.0, "session": "session-a", "input": 121},
            {"name": "measurement_end", "time": 131.2, "session": "session-a"},
        ])
        ready = ready_record()
        identity_map = expected_identity()
        args = (trace, ready, {**ready, "time": 8.8}, visibility_samples(9.0, 131.2), identity_map, "n-1")
        duplicate = {"name": "input", "time": 10.0, "session": "session-a", "input": 1}
        trace["events"].append(duplicate)
        with self.assertRaisesRegex(ValueError, "duplicate in-interval input"):
            visible.validate_qualified_interval(*args)
        trace["events"].pop()
        callback = next(event for event in trace["events"]
                        if event.get("name") == "callback_enqueue" and event.get("input") == 1)
        callback["time"] = float("nan")
        with self.assertRaisesRegex(ValueError, "callback enqueue"):
            visible.validate_qualified_interval(*args)

    def test_qualified_interval_rejects_missing_frame_identity_and_app_failure_marker(self):
        trace = causal_trace(121)
        trace["events"] = [event for event in trace["events"]
            if not (event.get("name") in ("job_dequeue", "callback_enqueue", "drawable_presented")
                    and event.get("input") in range(2, 23))]
        next(event for event in trace["events"] if event.get("name") == "job_dequeue"
             and event.get("input") == 1).pop("generation")
        trace["events"].extend([
            {"name": "measurement_start", "time": 9.0, "session": "session-a"},
            {"name": "input_sequence_complete", "time": 131.0, "session": "session-a", "input": 121},
            {"name": "measurement_end", "time": 131.2, "session": "session-a"},
        ])
        ready = ready_record()
        args = (trace, ready, {**ready, "time": 8.8}, visibility_samples(9.0, 131.2),
                expected_identity(), "n-1")
        with self.assertRaisesRegex(ValueError, "fewer than 100"):
            visible.validate_qualified_interval(*args)
        trace["events"].append({"name": "qualification_failed", "time": 131.1, "session": "session-a"})
        with self.assertRaisesRegex(ValueError, "qualification failure"):
            visible.validate_qualified_interval(*args)

    def test_qualified_interval_rejects_stale_identity_and_bad_visibility_sampling(self):
        trace = causal_trace(121)
        trace["events"].extend([
            {"name": "measurement_start", "time": 9.0, "session": "session-a"},
            {"name": "input_sequence_complete", "time": 131.0, "session": "session-a", "input": 121},
            {"name": "measurement_end", "time": 131.2, "session": "session-a"},
        ])
        ready = ready_record()
        samples = visibility_samples(9.0, 131.2)
        identity = expected_identity()
        args = (trace, ready, {**ready, "time": 8.8}, samples, identity)
        with self.assertRaisesRegex(ValueError, "nonce"):
            visible.validate_qualified_interval(*args, expected_nonce="stale")
        with self.assertRaisesRegex(ValueError, "sampling gap"):
            visible.validate_qualified_interval(trace, ready, {**ready, "time": 8.8},
                [samples[0], samples[1], samples[-1]], identity, "n-1")
        out_of_order = list(samples)
        out_of_order[10], out_of_order[11] = out_of_order[11], out_of_order[10]
        with self.assertRaisesRegex(ValueError, "monotonic acquisition"):
            visible.validate_qualified_interval(trace, ready, {**ready, "time": 8.8},
                out_of_order, identity, "n-1")
        dense = visibility_samples(9.0, 131.2)
        dense[1]["pid"] = 43
        with self.assertRaisesRegex(ValueError, "identity"):
            visible.validate_qualified_interval(trace, ready, {**ready, "time": 8.8}, dense, identity, "n-1")

    def test_launch_redirection_targets_exist_before_launch(self):
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory) / "output"
            output.mkdir()
            stdio_directory, stdout, stderr = visible.prepare_launch_stdio(output)
            self.assertEqual(stdio_directory, output)
            self.assertTrue(stdout.is_file())
            self.assertTrue(stderr.is_file())
            self.assertEqual(stdout.read_bytes(), b"")
            self.assertEqual(stderr.read_bytes(), b"")
            relay = Path(directory) / "relay"
            stdio_directory, stdout, stderr = visible.prepare_launch_stdio(output, relay)
            self.assertEqual(stdio_directory, relay)
            self.assertTrue(stdout.is_file())
            self.assertTrue(stderr.is_file())
            self.assertEqual(stdout.read_bytes(), b"")
            self.assertEqual(stderr.read_bytes(), b"")

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
        next(event for event in trace["events"] if event["name"] == "drawable_presented")["time"] = 0
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

    def test_process_identity_includes_one_concrete_launch(self):
        identity = {"pid": 42, "bundle_id": "dev.tessera.test", "bundle_url": "/tmp/Test.app",
                    "launch_date": 1_800_000_000.0}
        self.assertEqual(visible.validate_process_identity(identity, None), identity)
        replacement = {**identity, "pid": 43}
        with self.assertRaisesRegex(ValueError, "identity changed"):
            visible.validate_process_identity(replacement, identity)
        reused_pid = {**identity, "launch_date": 1_800_000_001.0}
        with self.assertRaisesRegex(ValueError, "identity changed"):
            visible.validate_process_identity(reused_pid, identity)
        missing_date = {key: value for key, value in identity.items() if key != "launch_date"}
        with self.assertRaisesRegex(ValueError, "launch date"):
            visible.validate_process_identity(missing_date, None)

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
