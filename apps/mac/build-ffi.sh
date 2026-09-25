#!/bin/bash
# Build the host macOS archive (arm64 on Apple Silicon). --universal adds Intel.
set -euo pipefail
MAC="$(cd "$(dirname "$0")" && pwd)"
ROOT="$(cd "$MAC/../.." && pwd)"
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$HOME/.cache/tessera-target/mac-ffi}"
export MACOSX_DEPLOYMENT_TARGET=15.0
case "$CARGO_TARGET_DIR" in "$ROOT"/*) printf 'CARGO_TARGET_DIR must be outside the checkout\n' >&2; exit 1;; esac
cd "$ROOT"
mkdir -p "$MAC/build/ffi" "$MAC/Sources/TesseraFFI" "$MAC/Sources/CTesseraFFI"
cargo build --locked --release -p tessera-ffi
cargo run --locked --release -p tessera-ffi --bin uniffi-bindgen -- generate \
  --library "$CARGO_TARGET_DIR/release/libtessera_ffi.dylib" --language swift \
  --out-dir "$MAC/build/ffi"
cp "$MAC/build/ffi/TesseraFFI.swift" "$MAC/Sources/TesseraFFI/"
cp "$MAC/build/ffi/CTesseraFFI.h" "$MAC/Sources/CTesseraFFI/"
cp "$MAC/build/ffi/CTesseraFFI.modulemap" "$MAC/Sources/CTesseraFFI/module.modulemap"
cp "$CARGO_TARGET_DIR/release/libtessera_ffi.a" "$MAC/build/ffi/libtessera_ffi.a"
if [[ "${1:-}" == "--universal" ]]; then
  for arch in aarch64-apple-darwin x86_64-apple-darwin; do
    cargo build --locked --release -p tessera-ffi --lib --target "$arch"
  done
  lipo -create "$CARGO_TARGET_DIR/aarch64-apple-darwin/release/libtessera_ffi.a" \
    "$CARGO_TARGET_DIR/x86_64-apple-darwin/release/libtessera_ffi.a" -output "$MAC/build/ffi/libtessera_ffi.a"
fi
lipo -info "$MAC/build/ffi/libtessera_ffi.a"
