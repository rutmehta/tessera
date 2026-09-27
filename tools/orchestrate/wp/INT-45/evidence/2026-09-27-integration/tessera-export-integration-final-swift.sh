#!/bin/bash
set -euo pipefail
cd /Users/rutmehta/.codex/worktrees/export-integration/tessera
[ "$(git rev-parse HEAD)" = b82fe3330d01d5011b779bddb2555f5702befe89 ]
[ -z "$(git status --porcelain --untracked-files=no)" ]
[ -e fixtures/raw/sample.dng ]
swift test --package-path apps/mac --scratch-path /Volumes/betterSSD/tessera-cache/swift/export-integration -c release -Xswiftc -enable-testing > /tmp/tessera-export-integration-final-swift.log 2>&1
printf 'FINAL_SWIFT_GATE_OK source=b82fe3330d01d5011b779bddb2555f5702befe89\n'
