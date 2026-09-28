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
    trace_path = out / "trace.json"
    stdout_path, stderr_path = out / "app-stdout.log", out / "app-stderr.log"
    probe_source = Path(__file__).with_name("VisibleWindowProbe.swift")
    probe = out / "visible-window-probe"
    subprocess.run(["swiftc", str(probe_source), "-o", str(probe)], check=True, timeout=60)
    probe_hash = _digest(probe_source)
    probe_binary_hash = _digest(probe)
    initial_observation = _run_probe(probe, args.expected_bundle_id)
    if initial_observation.get("bundle_apps"):
        raise ValueError("the unique test bundle is already running")

    launch_id = str(uuid.uuid4())
    launch = ["open", "-n", "-a", str(app), "--stdout", str(stdout_path), "--stderr", str(stderr_path),
              "--args", "--timing-visible", "--timing-selftest", "--develop-selftest",
              "--timing-output", str(trace_path), "--app-dir", str(support),
              "--folder", str(fixture_dir)]
    preflight = {"launch_id": launch_id, "command": launch,
                 "initial_observation": initial_observation,
                 "bundle_id": args.expected_bundle_id,
                 "expected_commit": args.expected_commit, "app_url": str(app),
                 "fixture": str(copied_fixture), "fixture_sha256": _digest(copied_fixture),
                 "app_provenance": provenance, "probe_source_sha256": probe_hash,
                 "probe_binary_sha256": probe_binary_hash, "visible_mode": True,
                 "nonactivating": False, "app_support": str(support),
                 "startup_allowance_seconds": 5, "outer_deadline_seconds": 180}
    (out / "run.json").write_text(json.dumps(preflight, indent=2) + "\n")
    context = RunArtifacts(out, app, args.expected_bundle_id, launch_id)
    context.probe = probe
    observation_path = out / "window-observations.jsonl"
    try:
        context.launch_attempted = True
        subprocess.run(launch, check=True, timeout=15)
        test_pid = None
        pinned_identity = None
        foreground_seen = False
        window_seen = False
        first_pid_time = None
        observations = []
        deadline = time.monotonic() + 180
        receipt_path = trace_path.with_suffix(trace_path.suffix + ".window.json")
        with observation_path.open("w") as stream:
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
                elif test_pid is not None and not trace_path.exists():
                    raise ValueError("test app exited before writing its trace")
                time.sleep(.25)
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
        summary = validate_trace(trace)
        summary.update({"provenance": {key: provenance[key] for key in
                                       ("commit", "configuration", "source_sha256", "archive_sha256", "bindings")},
                        "launch_id": launch_id, "bundle_id": args.expected_bundle_id,
                        "bundle_url": str(app), "process_id": test_pid,
                        "process_launch_date": pinned_identity["launch_date"],
                        "window_number": target["window_id"], "window_receipt": receipt,
                        "foreground_observations": len(observations),
                        "foreground_samples_for_test_app": len(foreground_samples),
                        "foreground_seen": foreground_seen, "visible_window_seen": window_seen,
                        "visibility_limit": "Window-server ordering/overlap and NSWindow occlusion state are evidence of a visible frontmost window, not a pixel-perfect guarantee that every pixel was unobscured.",
                        "measurement_limit": "Foreground samples corroborate the isolated self-test; they do not bound the P01 measured interval because the self-test starts before sampling begins.",
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
