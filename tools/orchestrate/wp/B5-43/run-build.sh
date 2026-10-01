#!/bin/bash
set -euo pipefail
cd "$(dirname "$0")/../../../.."
export PATH="$HOME/.cargo/bin:$PATH"
export CARGO_TARGET_DIR="$HOME/.cache/tessera-target/B5-43"
# Wait for already-running builds on the shared machine; never stop another job.
while ps -axo comm= | grep -Eq '/(cargo|swift-frontend|swift-build|swift-test)$'; do sleep 10; done
cd apps/mac
./build-ffi.sh
swift test -c release -Xswiftc -enable-testing --filter SelfTestHostTests.testLiveDocumentFilterSheetResizeP19
