#!/usr/bin/env bash
# Runs this checkout's document self-tests; preserves each run's logs and screenshots.
# Optional SP/EV override scratch/evidence parents. --root explicitly selects another checkout.
set -euo pipefail
exec python3 -B "$(cd "$(dirname "$0")" && pwd)/selftest_runner.py" "$@"
