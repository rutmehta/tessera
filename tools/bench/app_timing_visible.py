#!/usr/bin/env python3
"""Visible foreground corroboration for the P01 trace; not a benchmark interval."""
import argparse
import json
import math
import os
from pathlib import Path
import plistlib
import shutil
import subprocess
import sys
import time
import uuid

ROOT = Path(__file__).resolve().parents[2]
sys.dont_write_bytecode = True
sys.path.insert(0, str(Path(__file__).parent))
import app_timing


def validate_trace(trace):
    summary = app_timing.summarize(trace)
    presented = [event for event in trace.get("events", [])
                 if event.get("name") == "drawable_presented"]
    if any(not math.isfinite(event.get("time", float("nan"))) or event["time"] <= 0
           for event in presented):
        raise ValueError("drawable_presented contains a missing, non-finite, or non-positive actual timestamp")
    if summary["dropped"] != 0:
        raise ValueError("trace dropped records")
    if summary["causally_presented_inputs"] < 100:
        raise ValueError(f"only {summary['causally_presented_inputs']} inputs have causal actual-present joins")
    if not summary["p01_complete"]:
        raise ValueError("existing causal P01 oracle is incomplete")
    return summary


def _finite_timestamp(value, label):
    if (not isinstance(value, (int, float)) or isinstance(value, bool)
            or not math.isfinite(value) or value <= 0):
        raise ValueError(f"{label} has a missing, non-finite, or non-positive timestamp")
    return float(value)


class StableForegroundDwell:
    """Require at least 500 ms of good probes, with no observed gap over 250 ms."""
    def __init__(self, duration=0.5, max_gap=0.250):
        self.duration = duration
        self.max_gap = max_gap
        self.first = None
        self.last = None
        self.samples = 0
        self.maximum_gap = 0.0

    def observe(self, sample_time):
        sample_time = _finite_timestamp(sample_time, "foreground dwell probe")
        if self.last is None or sample_time <= self.last or sample_time - self.last > self.max_gap:
            self.first = sample_time
            self.samples = 1
            self.maximum_gap = 0.0
        else:
            self.samples += 1
            self.maximum_gap = max(self.maximum_gap, sample_time - self.last)
        self.last = sample_time
        return self.samples >= 2 and self.last - self.first >= self.duration


def validate_qualified_interval(trace, ready, permit, samples, identity, expected_nonce,
                                minimum_inputs=100, max_sample_gap=0.250):
    """Validate a nonce-bound, same-process/window/session P01 measurement interval.

    This is deliberately stricter than the legacy background trace summary. It rejects
    ambiguous event identities instead of relying on dict overwrite/first-event behavior.
    """
    if trace.get("dropped") != 0:
        raise ValueError("trace dropped records")
    if any(event.get("name") == "qualification_failed" for event in trace.get("events", [])):
        raise ValueError("app recorded a qualification failure")
    required = {"nonce": expected_nonce, **identity}
    for label, record in (("ready", ready), ("start permit", permit)):
        for key, expected in required.items():
            if record.get(key) != expected:
                raise ValueError(f"{label} {key} identity does not match the qualified run")
        _finite_timestamp(record.get("time"), f"{label} record")
    ready_time = float(ready["time"])
    permit_time = float(permit["time"])
    if permit_time < ready_time:
        raise ValueError("start permit predates app readiness")

    events = trace.get("events", [])
    markers = {}
    for name in ("measurement_start", "input_sequence_complete", "measurement_end"):
        matches = [event for event in events if event.get("name") == name]
        if len(matches) != 1:
            raise ValueError(f"expected exactly one {name} marker")
        marker = matches[0]
        if marker.get("session") != identity["session"]:
            raise ValueError(f"{name} marker has the wrong session identity")
        markers[name] = _finite_timestamp(marker.get("time"), name)
    start, sequence_done, end = (markers["measurement_start"],
                                 markers["input_sequence_complete"],
                                 markers["measurement_end"])
    if not (permit_time <= start < sequence_done <= end):
        raise ValueError("measurement markers are out of order or outside the permitted interval")

    input_events = [event for event in events if event.get("name") == "input"
                    and event.get("session") == identity["session"]
                    and start <= _finite_timestamp(event.get("time"), "input event") <= end]
    inputs = {}
    for event in input_events:
        input_id = event.get("input")
        if not isinstance(input_id, int) or isinstance(input_id, bool):
            raise ValueError("in-interval input has no valid input identity")
        if input_id in inputs:
            raise ValueError("duplicate in-interval input identity")
        inputs[input_id] = event

    sequence_marker = next(event for event in events if event.get("name") == "input_sequence_complete")
    final_input = sequence_marker.get("input")
    if not isinstance(final_input, int) or isinstance(final_input, bool) or final_input not in inputs:
        raise ValueError("final input is absent from the measured interval")
    if len(inputs) != 121:
        raise ValueError(f"the fixed input sequence must contain exactly 121 unique inputs; found {len(inputs)}")
    ordered_ids = sorted(inputs)
    if ordered_ids != list(range(ordered_ids[0], ordered_ids[0] + 121)) or final_input != ordered_ids[-1]:
        raise ValueError("the fixed input sequence is not 121 consecutive inputs ending at the final input")
    if sequence_done < _finite_timestamp(inputs[final_input].get("time"), "final input"):
        raise ValueError("input sequence completion predates the final input")
    ordered_input_times = [_finite_timestamp(inputs[input_id].get("time"), "input event")
                           for input_id in ordered_ids]
    if any(value > sequence_done for value in ordered_input_times) or ordered_input_times != sorted(ordered_input_times):
        raise ValueError("input events are out of order or occur after the fixed sequence completes")
    app_checks = [event for event in events if event.get("name") == "app_visibility_check"
                  and event.get("session") == identity["session"]
                  and start <= _finite_timestamp(event.get("time"), "app visibility check") <= sequence_done]
    checks_by_ordinal = {}
    for event in app_checks:
        span = event.get("span")
        prefix = "scripted-input-"
        try:
            ordinal = int(span[len(prefix):]) if isinstance(span, str) and span.startswith(prefix) else None
        except ValueError:
            ordinal = None
        if ordinal is None or ordinal in checks_by_ordinal:
            raise ValueError("app visibility checks have missing or duplicate sequence identity")
        checks_by_ordinal[ordinal] = event
    if set(checks_by_ordinal) != set(range(1, 122)):
        raise ValueError("each of the 121 scripted inputs requires one app-side visibility check")
    for ordinal, input_id in enumerate(ordered_ids, start=1):
        check_time = _finite_timestamp(checks_by_ordinal[ordinal].get("time"), "app visibility check")
        previous_input_time = start if ordinal == 1 else ordered_input_times[ordinal - 2]
        if not previous_input_time <= check_time <= ordered_input_times[ordinal - 1]:
            raise ValueError("app visibility check was outside its scripted input boundary")

    chain_count = 0
    final_presented = False
    input_to_present_ms = []
    for input_id, input_event in inputs.items():
        input_time = _finite_timestamp(input_event.get("time"), "input event")
        dequeues = [event for event in events if event.get("name") == "job_dequeue"
                    and event.get("session") == identity["session"] and event.get("input") == input_id]
        callbacks = [event for event in events if event.get("name") == "callback_enqueue"
                     and event.get("session") == identity["session"] and event.get("input") == input_id]
        valid_presentations = []
        for dequeue in dequeues:
            dequeue_time = _finite_timestamp(dequeue.get("time"), "job dequeue")
            if not start <= input_time <= dequeue_time <= end:
                continue
            frame_key = (dequeue.get("generation"), dequeue.get("level"))
            if (not isinstance(frame_key[0], int) or isinstance(frame_key[0], bool)
                    or not isinstance(frame_key[1], int) or isinstance(frame_key[1], bool)):
                continue
            matching_callbacks = [event for event in callbacks
                                  if (event.get("generation"), event.get("level")) == frame_key]
            if not matching_callbacks:
                continue
            for callback in matching_callbacks:
                callback_time = _finite_timestamp(callback.get("time"), "callback enqueue")
                if callback.get("residency") not in ("resident", "fallback") or not dequeue_time <= callback_time <= end:
                    continue
                matching_presentations = [event for event in events if event.get("name") == "drawable_presented"
                                          and event.get("session") == identity["session"]
                                          and event.get("input") == input_id
                                          and (event.get("generation"), event.get("level")) == frame_key]
                for presented in matching_presentations:
                    present_time = _finite_timestamp(presented.get("time"), "actual presentation")
                    if callback_time <= present_time <= end:
                        valid_presentations.append((present_time, callback, dequeue, presented))
        if valid_presentations:
            # One input may refine through several frame generations. Count its earliest
            # positive actual presentation, matching the legacy analyzer's one-input rule.
            chain_count += 1
            earliest_presentation = min(valid_presentations, key=lambda chain: chain[0])[0]
            input_to_present_ms.append((earliest_presentation - input_time) * 1000)
            if input_id == final_input:
                final_presented = True
    if not final_presented:
        raise ValueError("final input lacks an actual presentation inside the measurement end")
    if chain_count < minimum_inputs:
        raise ValueError(f"fewer than {minimum_inputs} complete causal presentations inside the interval")

    sample_times = [_finite_timestamp(sample.get("time"), "visibility sample") for sample in samples]
    if sample_times != sorted(sample_times):
        raise ValueError("visibility samples are not in monotonic acquisition order")
    bracketing = [sample for sample in samples if start <= float(sample["time"]) <= end]
    before = [sample for sample in samples if float(sample["time"]) < start]
    after = [sample for sample in samples if float(sample["time"]) > end]
    if not before or not after or not bracketing:
        raise ValueError("visibility samples do not bracket the full measurement interval")
    relevant = [before[-1], *bracketing, after[0]]
    max_gap = 0.0
    for index, sample in enumerate(relevant):
        if any(sample.get(key) != expected for key, expected in identity.items()):
            raise ValueError("visibility sample process/window identity changed")
        if sample.get("nonce") != expected_nonce:
            raise ValueError("visibility sample nonce does not match the qualified run")
        if sample.get("frontmost") is not True or sample.get("visible") is not True:
            raise ValueError("visibility sample did not confirm foreground visible window")
        if index:
            gap = float(sample["time"]) - float(relevant[index - 1]["time"])
            max_gap = max(max_gap, gap)
            if gap <= 0 or gap > max_sample_gap:
                raise ValueError("visibility sampling gap exceeds the qualified maximum")
    return {"measurement_start": start, "measurement_end": end,
            "causal_presentations": chain_count, "in_interval_inputs": len(inputs),
            "app_side_visibility_checks": len(checks_by_ordinal),
            "final_input": final_input, "visibility_samples": len(bracketing),
            "maximum_observed_sample_gap_seconds": max_gap,
            "input_to_present_ms": input_to_present_ms,
            "input_to_present_p50_ms": app_timing.percentile(input_to_present_ms, .5),
            "input_to_present_p95_ms": app_timing.percentile(input_to_present_ms, .95),
            "visibility_sample_target_seconds": 0.050,
            "qualified_interval": True}


def _intersects(a, b):
    return (a["x"] < b["x"] + b["width"] and a["x"] + a["width"] > b["x"]
            and a["y"] < b["y"] + b["height"] and a["y"] + a["height"] > b["y"])


def validate_window_observation(observation, expected_pid, expected_window_id=None):
    """Require frontmost app and an on-screen normal window; reject detected foreign overlap."""
    if observation.get("frontmost_pid") != expected_pid:
        raise ValueError("test app was not the foreground process")
    windows = observation.get("windows", [])
    candidates = [window for window in windows
                  if window.get("owner_pid") == expected_pid and window.get("layer") == 0
                  and window.get("onscreen")]
    if expected_window_id is not None:
        candidates = [window for window in candidates if window.get("window_id") == expected_window_id]
    if not candidates:
        raise ValueError("the expected regular app window was not reported on screen")
    target = candidates[0]
    target_index = windows.index(target)
    content = target.get("content_bounds", target.get("bounds"))
    for window in windows[:target_index]:
        if window.get("owner_pid") == expected_pid or window.get("layer", 0) != 0:
            continue
        if window.get("alpha", 1) > 0 and _intersects(content, window["bounds"]):
            raise ValueError("a reported foreign window overlaps the test window content")
    return target


def validate_bundle_identity(bundle_id, expected_bundle_id):
    if not expected_bundle_id or expected_bundle_id == "dev.tessera.app":
        raise ValueError("the visible capability run requires a unique test-app bundle identifier")
    if bundle_id != expected_bundle_id:
        raise ValueError("app bundle identity does not match --expected-bundle-id")


def validate_provenance(metadata, expected_commit):
    if metadata.get("commit") != expected_commit:
        raise ValueError("app provenance commit differs from the requested source commit")
    if metadata.get("configuration") != "release":
        raise ValueError("visible capability requires a Release app")


def validate_process_identity(application, expected):
    """Pin one concrete launch; bundle identity or runner UUID alone is insufficient."""
    launch_date = application.get("launch_date")
    if (not isinstance(launch_date, (int, float)) or isinstance(launch_date, bool)
            or not math.isfinite(launch_date) or launch_date <= 0):
        raise ValueError("running test app has no valid launch date")
    actual = {"pid": int(application["pid"]), "bundle_id": application.get("bundle_id"),
              "bundle_url": application.get("bundle_url"), "launch_date": launch_date}
    if expected is not None and actual != expected:
        raise ValueError("test app process identity changed during the capability run")
    return actual


def _digest(path):
    import hashlib
    h = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            h.update(chunk)
    return h.hexdigest()


def prepare_launch_redirection_targets(stdout_path, stderr_path):
    """Create fresh stdout/stderr files before Launch Services starts the app."""
    for path in (stdout_path, stderr_path):
        Path(path).touch(exist_ok=False)


def prepare_launch_stdio(output_directory, relay_directory=None):
    output_directory = Path(output_directory)
    if relay_directory is None:
        stdio_directory = output_directory
    else:
        stdio_directory = Path(relay_directory)
        stdio_directory.mkdir(parents=True, exist_ok=False)
    stdout_path = stdio_directory / "app-stdout.log"
    stderr_path = stdio_directory / "app-stderr.log"
    prepare_launch_redirection_targets(stdout_path, stderr_path)
    return stdio_directory, stdout_path, stderr_path


def _same_path(left, right):
    return Path(left).resolve() == Path(right).resolve()


def _run_probe(probe, bundle_id, timeout=5):
    result = subprocess.run([str(probe), bundle_id], check=True, capture_output=True,
                            text=True, timeout=timeout)
    return json.loads(result.stdout)


def validate_visibility_sample(observation, pid, elapsed, foreground_seen, window_seen,
                               startup_allowance=5.0):
    frontmost = observation.get("frontmost_pid") == pid
    if foreground_seen and not frontmost:
        raise ValueError("test app lost foreground during the capability sample")
    candidates = [window for window in observation.get("windows", [])
                  if window.get("owner_pid") == pid and window.get("layer") == 0
                  and window.get("onscreen")]
    if window_seen and not candidates:
        raise ValueError("test window disappeared from the on-screen window list during measurement")
    if elapsed > startup_allowance and (not frontmost or not candidates):
        raise ValueError("test app did not reach a foreground visible regular window within startup allowance")
    target = None
    if frontmost and candidates:
        target = validate_window_observation(observation, pid)
    return foreground_seen or frontmost, window_seen or bool(candidates), target


class RunArtifacts:
    def __init__(self, out, app, bundle_id, launch_id):
        self.out = out
        self.app = app
        self.bundle_id = bundle_id
        self.launch_id = launch_id
        self.probe = None
        self.launch_attempted = False
        self.owned = None
        self.success = False

    def write_json(self, name, value):
        (self.out / name).write_text(json.dumps(value, indent=2) + "\n")

    def record_owned(self, application, reason):
        self.owned = {"launch_id": self.launch_id,
                      "pid": int(application["pid"]),
                      "bundle_id": self.bundle_id,
                      "bundle_url": application["bundle_url"],
                      "launch_date": application["launch_date"],
                      "ownership_basis": reason}
        self.write_json("owned-app.json", self.owned)

    def _discover_after_open_abort(self):
        """Find only the new, exact-URL process when the bounded `open` call itself failed."""
        if self.owned or not self.launch_attempted or not self.probe:
            return
        deadline = time.monotonic() + 2
        while time.monotonic() < deadline:
            try:
                observation = _run_probe(self.probe, self.bundle_id, timeout=3)
            except (OSError, ValueError, subprocess.SubprocessError):
                return
            exact = [app for app in observation.get("bundle_apps", [])
                     if app.get("bundle_id") == self.bundle_id
                     and app.get("bundle_url") and _same_path(app["bundle_url"], self.app)
                     and isinstance(app.get("launch_date"), (int, float))
                     and math.isfinite(app["launch_date"]) and app["launch_date"] > 0]
            if len(exact) == 1 and len(observation.get("bundle_apps", [])) == 1:
                self.record_owned(exact[0], "unique bundle absent before launch; exact app URL appeared after this run's open request")
                return
            if observation.get("bundle_apps"):
                return
            time.sleep(.2)

    def cleanup_owned_after_failure(self):
        if self.success:
            return
        self._discover_after_open_abort()
        if not self.owned or not self.probe:
            return
        cleanup = {**self.owned, "graceful_terminate_attempted": True}
        try:
            before = _run_probe(self.probe, self.bundle_id, timeout=3)
            matches = [app for app in before.get("bundle_apps", [])
                       if int(app["pid"]) == self.owned["pid"]
                       and app.get("bundle_id") == self.bundle_id
                       and app.get("bundle_url") == self.owned["bundle_url"]
                       and app.get("launch_date") == self.owned["launch_date"]]
            if not matches:
                cleanup["graceful_terminate_attempted"] = False
                cleanup["process_already_exited_or_identity_changed"] = True
                self.write_json("cleanup.json", cleanup)
                return
            command = [str(self.probe), "--terminate", str(self.owned["pid"]),
                       self.bundle_id, self.owned["bundle_url"], str(self.owned["launch_date"])]
            response = subprocess.run(command, check=True, capture_output=True, text=True, timeout=5)
            cleanup["terminate_response"] = json.loads(response.stdout)
            deadline = time.monotonic() + 5
            while time.monotonic() < deadline:
                observation = _run_probe(self.probe, self.bundle_id, timeout=3)
                still_running = any(int(app["pid"]) == self.owned["pid"]
                                    and app.get("bundle_id") == self.bundle_id
                                    and app.get("bundle_url") == self.owned["bundle_url"]
                                    and app.get("launch_date") == self.owned["launch_date"]
                                    for app in observation.get("bundle_apps", []))
                if not still_running:
                    cleanup["process_exited"] = True
                    break
                time.sleep(.25)
            else:
                cleanup["process_exited"] = False
                cleanup["note"] = "Graceful terminate was requested; no force kill was attempted."
        except (OSError, ValueError, KeyError, subprocess.SubprocessError) as error:
            cleanup["error"] = str(error)
            cleanup["note"] = "Cleanup failed safely; no other process was targeted."
        self.write_json("cleanup.json", cleanup)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--app", type=Path, required=True, help="already signed, provenance-pinned unique test app")
    parser.add_argument("--expected-bundle-id", required=True)
    parser.add_argument("--expected-commit", required=True)
    parser.add_argument("--fixture", type=Path, required=True, help="one RAW fixture copied into this run")
    parser.add_argument("--output", type=Path, required=True, help="fresh disposable run directory")
    parser.add_argument("--stdio-relay-directory", type=Path,
                        help="fresh caller-owned directory for Launch Services stdout/stderr")
    args = parser.parse_args()

    app = args.app.resolve()
    out = args.output.resolve()
    fixture = args.fixture.resolve()
    if out.exists():
        raise ValueError("output directory must not already exist")
    info = plistlib.loads((app / "Contents/Info.plist").read_bytes())
    validate_bundle_identity(info.get("CFBundleIdentifier"), args.expected_bundle_id)
    subprocess.run([sys.executable, str(ROOT / "apps/mac/Support/provenance.py"),
                    "verify", "--app", str(app)], check=True, timeout=30)
    subprocess.run(["codesign", "--verify", "--deep", "--strict", str(app)], check=True, timeout=30)
    provenance = json.loads((app / "Contents/Resources/build-provenance.json").read_text())
    validate_provenance(provenance, args.expected_commit)
    if not fixture.is_file():
        raise ValueError("fixture does not exist")
    if "TESSERA_DOC_FRAME_LOG" in os.environ or "TESSERA_SELFTEST_VERBOSE" in os.environ:
        raise ValueError("frame/self-test verbose logging must be disabled")

    out.mkdir(parents=True)
    fixture_dir = out / "fixture"
    fixture_dir.mkdir()
    copied_fixture = fixture_dir / fixture.name
    shutil.copy2(fixture, copied_fixture)
    if _digest(copied_fixture) != _digest(fixture):
        raise ValueError("copied fixture hash mismatch")
    support = out / "app-support"
    control_dir = out / "timing-control"
    control_dir.mkdir()
    trace_path = out / "trace.json"
    stdio_directory, stdout_path, stderr_path = prepare_launch_stdio(out, args.stdio_relay_directory)
    probe_source = Path(__file__).with_name("VisibleWindowProbe.swift")
    probe = out / "visible-window-probe"
    subprocess.run(["swiftc", str(probe_source), "-o", str(probe)], check=True, timeout=60)
    probe_hash = _digest(probe_source)
    probe_binary_hash = _digest(probe)
    initial_observation = _run_probe(probe, args.expected_bundle_id)
    if initial_observation.get("bundle_apps"):
        raise ValueError("the unique test bundle is already running")

    launch_id = str(uuid.uuid4())
    run_nonce = str(uuid.uuid4())
    launch = ["open", "-n", "-a", str(app), "--stdout", str(stdout_path), "--stderr", str(stderr_path),
              "--args", "--timing-visible", "--timing-selftest", "--develop-selftest",
              "--timing-output", str(trace_path), "--timing-control-dir", str(control_dir),
              "--timing-nonce", run_nonce, "--app-dir", str(support),
              "--folder", str(fixture_dir)]
    preflight = {"launch_id": launch_id, "command": launch,
                 "initial_observation": initial_observation,
                 "bundle_id": args.expected_bundle_id,
                 "expected_commit": args.expected_commit, "app_url": str(app),
                 "fixture": str(copied_fixture), "fixture_sha256": _digest(copied_fixture),
                 "control_directory": str(control_dir), "run_nonce": run_nonce,
                 "app_provenance": provenance, "probe_source_sha256": probe_hash,
                 "probe_binary_sha256": probe_binary_hash,
                 "runner_source_sha256": _digest(Path(__file__)),
                 "stdio_directory": str(stdio_directory), "stdout_path": str(stdout_path),
                 "stderr_path": str(stderr_path), "visible_mode": True,
                 "nonactivating": False, "app_support": str(support),
                 "startup_allowance_seconds": 5, "outer_deadline_seconds": 180}
    (out / "run.json").write_text(json.dumps(preflight, indent=2) + "\n")
    context = RunArtifacts(out, app, args.expected_bundle_id, launch_id)
    context.probe = probe
    observation_path = out / "window-observations.jsonl"
    sample_path = out / "visibility-samples.jsonl"
    try:
        context.launch_attempted = True
        subprocess.run(launch, check=True, timeout=15)
        test_pid = None
        pinned_identity = None
        foreground_seen = False
        window_seen = False
        first_pid_time = None
        observations = []
        ready_record = None
        start_permit = None
        visible_samples = []
        dwell = StableForegroundDwell()
        dwell_qualified = None
        deadline = time.monotonic() + 180
        receipt_path = trace_path.with_suffix(trace_path.suffix + ".window.json")
        with observation_path.open("w") as stream, sample_path.open("w") as sample_stream:
            while time.monotonic() < deadline and not trace_path.is_file():
                observation = _run_probe(probe, args.expected_bundle_id, timeout=5)
                elapsed = 0 if first_pid_time is None else time.monotonic() - first_pid_time
                record = {"elapsed_since_first_pid_seconds": elapsed, "observation": observation}
                stream.write(json.dumps(record, sort_keys=True) + "\n")
                stream.flush()
                observations.append(record)

                applications = observation.get("bundle_apps", [])
                if len(applications) > 1:
                    raise ValueError("more than one instance of the unique test bundle is running")
                if applications:
                    launched_app = applications[0]
                    if not launched_app.get("bundle_url") or not _same_path(launched_app["bundle_url"], app):
                        raise ValueError("running bundle URL differs from the verified test app")
                    observed_identity = validate_process_identity(launched_app, pinned_identity)
                    if observed_identity["bundle_id"] != args.expected_bundle_id:
                        raise ValueError("running app bundle identifier differs from the verified test app")
                    if context.owned is None:
                        context.record_owned(launched_app,
                                             "bundle absent at preflight; exact bundle ID and app URL observed after this run's launch")
                        pinned_identity = observed_identity
                        test_pid = pinned_identity["pid"]
                        first_pid_time = time.monotonic()
                        elapsed = 0
                    foreground_seen, window_seen, _ = validate_visibility_sample(
                        observation, test_pid, elapsed, foreground_seen, window_seen)
                    ready_path = control_dir / "ready.json"
                    if ready_path.is_file():
                        current_ready = json.loads(ready_path.read_text())
                        if ready_record is None:
                            ready_record = current_ready
                            if (ready_record.get("nonce") != run_nonce
                                    or ready_record.get("pid") != test_pid
                                    or ready_record.get("bundle_id") != args.expected_bundle_id
                                    or not ready_record.get("session")
                                    or not isinstance(ready_record.get("window_number"), int)
                                    or ready_record.get("window_number") <= 0
                                    or not math.isclose(float(ready_record.get("launch_date", -1)),
                                                        pinned_identity["launch_date"], rel_tol=0, abs_tol=1e-6)
                                    or not _same_path(ready_record.get("bundle_url", ""), app)):
                                raise ValueError("app ready record does not match this nonce/process/session run")
                            _finite_timestamp(ready_record.get("time"), "app ready record")
                        elif current_ready != ready_record:
                            raise ValueError("app readiness identity changed during qualification")
                        target = validate_window_observation(observation, test_pid,
                                                             ready_record["window_number"])
                        if start_permit is None:
                            sample_time = _finite_timestamp(observation.get("sample_time"), "window probe")
                            if dwell.observe(sample_time):
                                dwell_qualified = {"first_sample_time": dwell.first,
                                    "permit_sample_time": sample_time, "samples": dwell.samples,
                                    "maximum_observed_gap_seconds": dwell.maximum_gap}
                                start_permit = {**ready_record, "time": sample_time}
                                temporary_permit = control_dir / "start.json.tmp"
                                temporary_permit.write_text(json.dumps(start_permit, sort_keys=True) + "\n")
                                os.replace(temporary_permit, control_dir / "start.json")
                        if start_permit is not None:
                            sample_time = _finite_timestamp(observation.get("sample_time"), "window probe")
                            sample_record = {"time": sample_time, "pid": test_pid,
                                "bundle_id": args.expected_bundle_id, "bundle_url": str(app),
                                "launch_date": ready_record["launch_date"],
                                "window_number": target["window_id"], "session": ready_record["session"],
                                "nonce": run_nonce, "frontmost": True, "visible": bool(target["onscreen"])}
                            visible_samples.append(sample_record)
                            sample_stream.write(json.dumps(sample_record, sort_keys=True) + "\n")
                            sample_stream.flush()
                elif test_pid is not None and not trace_path.exists():
                    raise ValueError("test app exited before writing its trace")
                time.sleep(.05)
        if not trace_path.is_file():
            raise ValueError("timed out without the self-test trace")
        # The app writes the receipt synchronously before atomically publishing its trace.
        # Still wait boundedly for both artifacts so an interrupted filesystem write is explicit.
        receipt_deadline = time.monotonic() + 5
        while time.monotonic() < receipt_deadline and not receipt_path.is_file():
            time.sleep(.05)
        if not receipt_path.is_file():
            raise ValueError("trace was published without its preceding regular-window receipt")
        receipt = json.loads(receipt_path.read_text())
        if test_pid is None or receipt.get("processID") != test_pid:
            raise ValueError("window receipt process does not match the observed app instance")
        if not foreground_seen or not receipt.get("appActive") or not receipt.get("isKeyWindow"):
            raise ValueError("foreground/key-window evidence did not identify the active test app")
        if (receipt.get("activationPolicy") != "regular" or receipt.get("windowTitle") != "Tessera"
                or not receipt.get("isRegularWindow") or not receipt.get("isVisible")
                or not receipt.get("occlusionVisible")):
            raise ValueError("regular test window was not visible at self-test completion")
        content_frame = receipt["contentScreenFrame"]
        if content_frame.get("width", 0) <= 0 or content_frame.get("height", 0) <= 0:
            raise ValueError("visible window receipt has an empty content frame")
        matching_record = next((record for record in reversed(observations)
                                if record["observation"].get("frontmost_pid") == test_pid), None)
        if matching_record is None:
            raise ValueError("no foreground window-server observation for the test app")
        matching_observation = matching_record["observation"]
        target = validate_window_observation(matching_observation, test_pid, receipt.get("windowNumber"))
        foreground_samples = [record for record in observations
                              if record["observation"].get("frontmost_pid") == test_pid]
        if len(foreground_samples) < 2 or not window_seen:
            raise ValueError("too few foreground/on-screen samples to corroborate a visible run")
        trace = json.loads(trace_path.read_text())
        if ready_record is None or start_permit is None:
            raise ValueError("visible ready/start handshake did not complete")
        identity = {"pid": test_pid, "bundle_id": args.expected_bundle_id,
                    "bundle_url": ready_record["bundle_url"], "launch_date": ready_record["launch_date"],
                    "window_number": ready_record["window_number"], "session": ready_record["session"]}
        summary = validate_qualified_interval(trace, ready_record, start_permit, visible_samples,
                                             identity, run_nonce)
        summary.update({"provenance": {key: provenance[key] for key in
                                       ("commit", "configuration", "source_sha256", "archive_sha256", "bindings")},
                        "p01_interval_qualified": True,
                        "input_path": "scripted AppModel.setAdjustment Exposure sequence with explicit flushPending; not an OS mouse gesture",
                        "launch_id": launch_id, "bundle_id": args.expected_bundle_id,
                        "bundle_url": str(app), "process_id": test_pid,
                        "process_launch_date": pinned_identity["launch_date"],
                        "window_number": target["window_id"], "window_receipt": receipt,
                        "ready_record": ready_record, "start_permit": start_permit,
                        "foreground_dwell": dwell_qualified,
                        "foreground_observations": len(observations),
                        "foreground_samples_for_test_app": len(foreground_samples),
                        "foreground_seen": foreground_seen, "visible_window_seen": window_seen,
                        "visibility_limit": "Window-server ordering/overlap and NSWindow occlusion state are evidence of a visible frontmost window, not a pixel-perfect guarantee that every pixel was unobscured.",
                        "measurement_limit": "Only same-session causal input-to-positive-present joins inside the nonce-bound, foreground-sampled interval contribute to qualified P01 metrics.",
                        "grid_appeared": any(e.get("name") == "grid_appeared" for e in trace.get("events", [])),
                        "selftest_complete": any(e.get("name") == "selftest_complete" for e in trace.get("events", []))})
        if not summary["grid_appeared"] or not summary["selftest_complete"]:
            raise ValueError("timing self-test did not complete with the regular library grid mounted")
        context.write_json("summary.json", summary)
        context.success = True
        print(json.dumps(summary, indent=2))
        return 0
    except Exception as error:
        context.write_json("runner-failure.json", {"launch_id": launch_id,
                            "error_type": type(error).__name__, "error": str(error),
                            "owned_app": context.owned})
        raise
    finally:
        context.cleanup_owned_after_failure()


if __name__ == "__main__":
    try:
        sys.exit(main())
    except (OSError, ValueError, KeyError, TypeError, subprocess.SubprocessError) as error:
        print(f"Visible timing capability failed: {error}", file=sys.stderr)
        sys.exit(1)
