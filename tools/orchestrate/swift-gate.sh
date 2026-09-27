#!/usr/bin/env bash
# Swift gate that does not trust XCTest's "(N unexpected)" summary: that bracket counts only
# thrown errors, not assertion failures. Fail on any "error: -[" line or any "with N failures"
# where N>0 that is not an XCTExpectFailure (expected failures do not print "error: -[").
set -uo pipefail
root="$(git rev-parse --show-toplevel)"
[ -e "$root/fixtures/raw/sample.dng" ] || { echo "fixtures/raw missing in $root: symlink it (ln -s <main>/fixtures/raw fixtures/raw) or run fixtures/fetch.sh; raw-based tests would fail with 'file raw couldn't be opened'"; exit 2; }
cd "$root/apps/mac"
./build-ffi.sh >/dev/null 2>&1 || { echo "build-ffi failed"; exit 1; }
swift build 2>&1 | grep -E "error:|Build complete" | tail -3
log=$(mktemp); swift test -c release -Xswiftc -enable-testing >"$log" 2>&1; rc=$?
grep -E "Executed .* tests" "$log" | tail -1; grep -E "Test run with" "$log" | tail -1
fails=$(grep -E "error: -\[" "$log" | sed -E 's/.*-\[[A-Za-z]+\.([A-Za-z]+) ([A-Za-z_0-9]+)\].*/\1.\2/' | sort -u)
if [ -n "$fails" ] || [ $rc -ne 0 ]; then echo "SWIFT GATE FAILED (exit $rc):"; echo "$fails"; rm -f "$log"; exit 1; fi
echo "SWIFT GATE OK"; rm -f "$log"
