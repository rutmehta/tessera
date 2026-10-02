#!/bin/bash
# Run from the worktree root. Synthetic and repo RAW fixtures; no exclusions.
set -u
export PATH="$HOME/.cargo/bin:$PATH"
export CARGO_TARGET_DIR="$HOME/.cache/tessera-target/LR-1-point-color"
export CARGO_BUILD_JOBS=3
export RAYON_NUM_THREADS=3
logs=tools/orchestrate/wp/LR-1/LR-1c
packages=(-p import-lrcat -p engine-api -p pipeline-cpu -p pipeline-gpu -p image-core -p export -p sidecar -p merge -p tessera-mcp -p tessera-ffi)
cargo clean --release "${packages[@]}" > "$logs/gates-clean.log" 2>&1
clean_status=$?
cargo test --release --locked "${packages[@]}" --no-fail-fast > "$logs/gates-test.log" 2>&1
test_status=$?
cargo clippy --release --locked "${packages[@]}" --all-targets -- -D warnings > "$logs/gates-clippy.log" 2>&1
clippy_status=$?
cargo fmt --all -- --check > "$logs/gates-fmt.log" 2>&1
fmt_status=$?
printf 'clean=%s test=%s clippy=%s fmt=%s\n' "$clean_status" "$test_status" "$clippy_status" "$fmt_status" | tee "$logs/gates-status.txt"
((clean_status == 0 && test_status == 0 && clippy_status == 0 && fmt_status == 0))
