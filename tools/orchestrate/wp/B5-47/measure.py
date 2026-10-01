#!/usr/bin/env python3
"""Serial, unordered SelfTestHost controls/export; run after building release tests."""
import json
import os
from pathlib import Path
import subprocess
import sys
import time

package = Path(__file__).resolve().parent
root = package.parents[3]
label = sys.argv[1]
for index in range(1, 4):
    prefix = package / "evidence" / f"{label}-{index}"
    env = dict(os.environ, TESSERA_FILTER_PERF="1", TESSERA_EXPORT_BASELINES="1",
               TESSERA_EXPORT_TEST_TRACE=str(prefix) + ".trace.json")
    samples = []
    with open(str(prefix) + ".log", "w") as log:
        process = subprocess.Popen([
            "swift", "test", "-c", "release", "--skip-build", "--filter",
            "DocumentExportFlatTests.testSmartFilterFixtureExportBoundsMainSpansAndCoalescesProgress"
        ], cwd=root / "apps/mac", env=env, stdout=log, stderr=subprocess.STDOUT)
        while process.poll() is None:
            samples.append({"wall": time.time(), "load1": os.getloadavg()[0]})
            time.sleep(1)
    Path(str(prefix) + ".load.json").write_text(json.dumps(samples, indent=2) + "\n")
    print(f"{label}-{index}: exit {process.returncode}, load1 "
          f"{min(s['load1'] for s in samples):.2f}–{max(s['load1'] for s in samples):.2f}", flush=True)
