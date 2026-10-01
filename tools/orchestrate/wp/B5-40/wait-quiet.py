#!/usr/bin/env python3
"""Record 30-second host samples; require load1 < 5 and no cargo/swift-build."""
import datetime
import json
import os
import subprocess
import sys
import time

for attempt in range(61):
    processes = subprocess.check_output(['ps', '-axo', 'pid=,comm='], text=True)
    builds = [line.strip() for line in processes.splitlines()
              if os.path.basename(line.strip().split(None, 1)[-1]) in ('cargo', 'swift-build')]
    load = os.getloadavg()[0]
    sample = dict(time=datetime.datetime.now().astimezone().isoformat(), load1=load,
                  build_processes=builds, quiet=load < 5 and not builds)
    with open(sys.argv[1], 'a') as output:
        output.write(json.dumps(sample) + '\n')
    print(json.dumps(sample), flush=True)
    if sample['quiet']:
        sys.exit(0)
    if attempt < 60:
        time.sleep(30)
sys.exit(2)
