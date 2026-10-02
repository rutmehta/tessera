#!/usr/bin/env bash
set -euo pipefail
cd "$(git rev-parse --show-toplevel)"
export PATH="$HOME/.cargo/bin:$PATH"
export CARGO_TARGET_DIR="$HOME/.cache/tessera-target/LR-6-lens-blur"
export CARGO_BUILD_JOBS=3
export RAYON_NUM_THREADS=3
export RUST_TEST_THREADS=3
export LR6_RECIPE
LR6_RECIPE=$(mktemp /tmp/tessera-lr6-recipe.XXXXXX)
trap 'rm -f "$LR6_RECIPE"' EXIT
logs=tools/orchestrate/wp/LR-6/evidence
mkdir -p "$logs"
packages=(-p import-lrcat -p engine-api -p image-core -p mask-store -p sidecar -p merge -p pipeline-cpu -p ml-depth -p tessera-ffi)
# All ordinary tests, including the entire FFI suite: no command-line skips.
# Rust #[ignore] tests retain their upstream reasons (benchmarks, optional model
# inference, real catalog, or paired E2E). The synthetic paired E2E runs below.
cargo clean --release "${packages[@]}" > "$logs/lr6e-clean.log" 2>&1
set +e
cargo test --release --locked "${packages[@]}" --no-fail-fast -- --nocapture > "$logs/lr6e-test.log" 2>&1
test_status=$?
cargo test --release --locked -p image-core --test lr6_depth_import synthetic_import_to_cpu_render \
  -- --ignored --exact > "$logs/lr6e-e2e.log" 2>&1
e2e_status=$?
cargo clippy --release --locked "${packages[@]}" --all-targets -- -D warnings > "$logs/lr6e-clippy.log" 2>&1
clippy_status=$?
cargo fmt --all -- --check > "$logs/lr6e-fmt.log" 2>&1

fmt_status=$?
printf "test=%s e2e=%s clippy=%s fmt=%s\n" "$test_status" "$e2e_status" "$clippy_status" "$fmt_status" | tee "$logs/lr6e-gate-status.log"
exit $((test_status || e2e_status || clippy_status || fmt_status))
