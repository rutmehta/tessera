# Exported JPEG reimport and editing preview validation evidence

The report and manifest in `evidence/` capture the tested source SHA, current FFI and full Swift results, original tested Swift executable hash, signed package executable hashes, and bounded GUI checks. `build-ffi.log` and `swift-test.log` are accompanied by their direct exit files and source/FFI freeze files.

The small generated JPEG input/output and their XMP sidecars are retained under `fixtures/`. The Sony RAW binary is intentionally omitted; only its post-edit JSON recipe and XMP sidecar are retained so reviewers can inspect the saved adjustment values without adding a 16.6 MB duplicate photo.

No app bundles, Rust archives, caches, or raw pixel buffers are included. Relative references in the report to transient `/tmp` locations describe the original run; equivalent retained JPEG and sidecar files are in this directory.
