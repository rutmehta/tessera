#!/usr/bin/env bash
set -euo pipefail
# This repository path contains a colon; clear the inherited DYLD override so
# rustc can form its macOS library search path correctly.
unset DYLD_FALLBACK_LIBRARY_PATH
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$HOME/.cache/tessera-target/ci}"
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
# Keep performance checks from competing with other tests in the same binary.
RUST_TEST_THREADS=1 cargo test --workspace --release
cargo deny check licenses bans
