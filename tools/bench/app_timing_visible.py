#!/usr/bin/env python3
"""Visible-window P01 capability check; it never substitutes callback time for presentation."""
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


def _digest(path):
    import hashlib
    h = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            h.update(chunk)
    return h.hexdigest()


def _run_probe(probe, bundle_id):
    raw = subprocess.check_output([str(probe), bundle_id], text=True)
    return json.loads(raw)


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
                    "verify", "--app", str(app)], check=True)
    subprocess.run(["codesign", "--verify", "--deep", "--strict", str(app)], check=True)
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
    subprocess.run(["swiftc", str(probe_source), "-o", str(probe)], check=True)
    probe_hash = _digest(probe_source)
    initial_observation = _run_probe(probe, args.expected_bundle_id)
    if initial_observation.get("bundle_pids"):
        raise ValueError("the unique test bundle is already running")
    launch = ["open", "-n", "-a", str(app), "--stdout", str(stdout_path), "--stderr", str(stderr_path),
              "--args", "--timing-visible", "--timing-selftest", "--develop-selftest",
              "--timing-output", str(trace_path), "--app-dir", str(support),
              "--folder", str(fixture_dir)]
    preflight = {"command": launch, "initial_observation": initial_observation,
                 "bundle_id": args.expected_bundle_id,
                 "expected_commit": args.expected_commit, "fixture": str(copied_fixture),
                 "fixture_sha256": _digest(copied_fixture), "app_provenance": provenance,
                 "probe_source_sha256": probe_hash, "visible_mode": True,
                 "nonactivating": False, "app_support": str(support)}
    (out / "run.json").write_text(json.dumps(preflight, indent=2) + "\n")
    subprocess.run(launch, check=True)

    observations = []
    test_pid = None
    foreground_seen = False
    deadline = time.monotonic() + 180
    while time.monotonic() < deadline and not trace_path.exists():
        observation = _run_probe(probe, args.expected_bundle_id)
        pids = observation.get("bundle_pids", [])
        if len(pids) > 1:
            raise ValueError("more than one instance of the unique test bundle is running")
        if pids:
            test_pid = pids[0]
            if observation.get("frontmost_pid") == test_pid:
                foreground_seen = True
            elif foreground_seen:
                raise ValueError("test app lost foreground before trace completion")
            visible = [w for w in observation.get("windows", [])
                       if w.get("owner_pid") == test_pid and w.get("layer") == 0 and w.get("onscreen")]
            observation["visible_test_windows"] = visible
        observations.append(observation)
        time.sleep(.25)
    (out / "window-observations.json").write_text(json.dumps(observations, indent=2) + "\n")
    receipt_path = trace_path.with_suffix(trace_path.suffix + ".window.json")
    if not trace_path.is_file() or not receipt_path.is_file():
        raise ValueError("timed out without trace and regular-window receipt")
    receipt = json.loads(receipt_path.read_text())
    if test_pid is None or receipt.get("processID") != test_pid:
        raise ValueError("window receipt process does not match the observed app instance")
    if not foreground_seen or not receipt.get("appActive") or not receipt.get("isKeyWindow"):
        raise ValueError("foreground/key-window evidence did not identify the active test app")
    if (receipt.get("activationPolicy") != "regular" or receipt.get("windowTitle") != "Tessera"
            or not receipt.get("isRegularWindow") or not receipt.get("isVisible")
            or not receipt.get("occlusionVisible")):
        raise ValueError("regular test window was not visible at self-test completion")
    matching_observation = next((item for item in reversed(observations)
                                 if item.get("frontmost_pid") == test_pid), None)
    if matching_observation is None:
        raise ValueError("no foreground window-server observation for the test app")
    content_frame = receipt["contentScreenFrame"]
    if content_frame.get("width", 0) <= 0 or content_frame.get("height", 0) <= 0:
        raise ValueError("visible window receipt has an empty content frame")
    target = validate_window_observation(matching_observation, test_pid, receipt.get("windowNumber"))
    foreground_samples = [item for item in observations if item.get("frontmost_pid") == test_pid]
    if len(foreground_samples) < 2:
        raise ValueError("too few foreground samples to corroborate a visible run")
    trace = json.loads(trace_path.read_text())
    summary = validate_trace(trace)
    summary.update({"provenance": {key: provenance[key] for key in
                                   ("commit", "configuration", "source_sha256", "archive_sha256", "bindings")},
                    "bundle_id": args.expected_bundle_id, "process_id": test_pid,
                    "window_number": target["window_id"], "window_receipt": receipt,
                    "foreground_observations": len(observations),
                    "foreground_samples_for_test_app": len(foreground_samples),
                    "foreground_seen": foreground_seen,
                    "visibility_limit": "Window-server ordering/occlusion and key-window state are corroborating evidence, not a pixel-level proof that every window pixel was unobscured.",
                    "grid_appeared": any(e.get("name") == "grid_appeared" for e in trace.get("events", [])),
                    "selftest_complete": any(e.get("name") == "selftest_complete" for e in trace.get("events", []))})
    (out / "summary.json").write_text(json.dumps(summary, indent=2) + "\n")
    print(json.dumps(summary, indent=2))
    return 0 if summary["grid_appeared"] and summary["selftest_complete"] else 1


if __name__ == "__main__":
    try:
        sys.exit(main())
    except (OSError, ValueError, KeyError, TypeError, subprocess.CalledProcessError) as error:
        print(f"Visible timing capability failed: {error}", file=sys.stderr)
        sys.exit(1)
