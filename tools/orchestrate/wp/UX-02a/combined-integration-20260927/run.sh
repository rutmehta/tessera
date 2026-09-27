#!/bin/bash
set -euo pipefail
cd /Users/rutmehta/.codex/worktrees/export-integration/tessera
python3 - <<'CHECK'
from pathlib import Path
import hashlib,json,subprocess
m=json.loads(Path('/tmp/tessera-review-masks-integration/source.json').read_text())
assert subprocess.check_output(['git','rev-parse','HEAD'],text=True).strip()==m['commit']
assert not subprocess.check_output(['git','status','--porcelain','--untracked-files=no'],text=True).strip()
for p,h in m['files'].items(): assert hashlib.sha256(Path(p).read_bytes()).hexdigest()==h,p
CHECK
export MACOSX_DEPLOYMENT_TARGET=15.0
swift test --jobs 2 --package-path apps/mac --scratch-path /Volumes/betterSSD/tessera-cache/swift/export-integration -c release -Xswiftc -enable-testing
