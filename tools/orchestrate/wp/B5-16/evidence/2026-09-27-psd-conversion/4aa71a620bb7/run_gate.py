#!/usr/bin/env python3
"""Run the frozen, bounded PSD conversion gates only when the A slot is granted."""

import hashlib
import json
import os
from pathlib import Path
import signal
import subprocess
import time

REPO = Path("/Users/rutmehta/.codex/worktrees/export-integration/tessera")
HERE = Path(__file__).resolve().parent
MANIFEST = json.loads((HERE / "manifest.json").read_text())
GATES = [
    ("private", ["cargo", "test", "-p", "compositor", "--lib", "cancellation_tests"]),
    ("placed_private", ["cargo", "test", "-p", "compositor", "--lib", "psd::placed::tests"]),
    ("public", ["cargo", "test", "-p", "compositor", "--test", "psd_cancellation"]),
    ("psd", ["cargo", "test", "-p", "compositor", "--test", "psd"]),
    ("psd_channels", ["cargo", "test", "-p", "compositor", "--test", "psd_channels"]),
    ("psd_document", ["cargo", "test", "-p", "compositor", "--test", "psd_document"]),
    ("m5_26_psd", ["cargo", "test", "-p", "compositor", "--test", "m5_26_psd"]),
    ("style_psd", ["cargo", "test", "-p", "compositor", "--test", "style_psd"]),
    ("transform_psd", ["cargo", "test", "-p", "compositor", "--test", "transform_psd"]),
    ("text_vector_psd", ["cargo", "test", "-p", "compositor", "--test", "text_vector_psd"]),
    ("strict", ["cargo", "clippy", "-p", "compositor", "--all-targets", "--", "-D", "warnings"]),
]

env = os.environ.copy()
env.update(
    CARGO_TARGET_DIR="/Volumes/betterSSD/tessera-cache/target/main",
    CARGO_BUILD_JOBS="2",
    RAYON_NUM_THREADS="2",
    MACOSX_DEPLOYMENT_TARGET="15.0",
)

for name, command in GATES:
    assert subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=REPO, text=True).strip() == MANIFEST["base_head"]
    changed = subprocess.check_output(
        ["git", "status", "--porcelain", "--untracked-files=all"], cwd=REPO, text=True
    )
    assert {line[3:] for line in changed.splitlines()} == set(MANIFEST["files"]), changed
    for file, expected in MANIFEST["files"].items():
        assert hashlib.sha256((REPO / file).read_bytes()).hexdigest() == expected["sha256"], file
    started = time.time()
    with (HERE / f"{name}.log").open("wb") as log:
        process = subprocess.Popen(command, cwd=REPO, env=env, stdout=log, stderr=subprocess.STDOUT, start_new_session=True)
        try:
            code = process.wait(timeout=600)
        except subprocess.TimeoutExpired:
            os.killpg(process.pid, signal.SIGKILL)
            process.wait()
            code = "timeout"
    (HERE / f"{name}.json").write_text(json.dumps({"command": command, "exit": code, "started_unix": started, "elapsed_seconds": time.time() - started}, indent=2) + "\n")
    print(name, code, flush=True)
    if code != 0:
        break
