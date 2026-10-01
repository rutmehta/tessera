#!/usr/bin/env python3
"""Record 30-second host samples; require two quiet samples, bounded at 120 minutes."""
import datetime
import json
import os
import subprocess
import sys
import time

limit = float(os.environ.get("QUIET_LOAD_LIMIT", "5"))
consecutive = 0
for attempt in range(241):
    processes = subprocess.check_output(['ps', '-axo', 'pid=,comm='], text=True)
    builds = [line.strip() for line in processes.splitlines()
              if os.path.basename(line.strip().split(None, 1)[-1]) in ('cargo', 'swift-build', 'xcodebuild')]
    load = os.getloadavg()[0]
    sample = dict(time=datetime.datetime.now().astimezone().isoformat(), load1=load,
                  build_processes=builds, quiet=load < limit and not builds, load_limit=limit)
    with open(sys.argv[1], 'a') as output:
        output.write(json.dumps(sample) + '\n')
    print(json.dumps(sample), flush=True)
    consecutive = consecutive + 1 if sample['quiet'] else 0
    if consecutive >= 2:
        sys.exit(0)
    if attempt < 240:
        time.sleep(30)
sys.exit(2)
