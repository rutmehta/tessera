#!/usr/bin/env bash
set -euo pipefail
# This repository path contains a colon; clear the inherited DYLD override so
# rustc can form its macOS library search path correctly.
unset DYLD_FALLBACK_LIBRARY_PATH
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo deny check licenses bans
