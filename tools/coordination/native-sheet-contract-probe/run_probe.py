#!/usr/bin/env python3
"""A-only execution proposal. Never imports or invokes a compiler.

External watchdog remains effective if AppKit blocks the probe's main thread.
Its timeout is FAILURE, never synthetic native completion or detachment.
"""
import argparse
import json
import pathlib
import subprocess
import sys

parser = argparse.ArgumentParser()
parser.add_argument("binary", type=pathlib.Path)
parser.add_argument("scenario", choices=["queued-cancel", "parent-close", "end-then-parent-close", "close-handler-end"])
parser.add_argument("--order-out-in-completion", action="store_true")
args = parser.parse_args()
binary = args.binary.resolve(strict=True)
# Inherit stdout/stderr so native JSON lines survive a blocked process or timeout.
command = [str(binary), args.scenario]
if args.order_out_in_completion:
    command.append("--order-out-in-completion")
process = subprocess.Popen(command)
try:
    status = process.wait(timeout=15)
except subprocess.TimeoutExpired:
    print(json.dumps({"event": "external.watchdog", "passed": False,
                      "reason": "probe did not exit within 15 seconds; native result not inferred",
                      "pid": process.pid, "scenario": args.scenario}), flush=True)
    # Target this captured child only. No process-name/global modal cancellation.
    process.kill()
    process.wait()
    sys.exit(124)
print(json.dumps({"event": "process.exit", "returncode": status,
                  "scenario": args.scenario}), flush=True)
sys.exit(status if status >= 0 else 128 - status)
