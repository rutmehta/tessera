# M2-09c validation

Implemented a shared crop/straighten capability predicate and CPU delegation at the public GPU image-stage boundary. The isolated WGSL kernel remains crop/straighten-only. CPU owns extended geometry validation and results.

## Checkout facts

Contrary to the brief, this checkout's CPU geometry still rejects nonidentity orientation and constrain-crop. It supports manual transforms and Upright. Tests now compare the public CPU/GPU outcomes rather than assuming all extended settings are errors.

GeometrySettings and Op::Geometry do not carry lens settings/calibration. The CPU full renderer composes lens corrections separately through geometry_mapped. This change does not port that map or claim full-renderer lens parity. The optional composed WGSL port is deferred.

The existing renderer excludes nondefault geometry from its resident graph and materializes upstream output at the M2 host-image barrier. No new resident readback API is needed. OPERATORS.md documents synchronization/readback, CPU resampling, and re-upload costs.

## Verification

CARGO_TARGET_DIR remained /Users/rutmehta/.cache/tessera-target/M2-09c for all Cargo commands.

- RED: revised public routing regression failed before the fix because automatic Upright succeeded on CPU but returned Unsupported on GPU (red.log).
- GREEN: geometry_compute passed after the capability fallback.
- Final command executed exactly as a chained gate:
  `cargo test --workspace --release && cargo clippy -p pipeline-gpu --all-targets -- -D warnings && cargo fmt --check`
- Gate exit status: 0. Full output: validation.log.
- Workspace summaries: 528 passed, 0 failed, 13 ignored across 150 test summaries (including doc tests).
- Geometry coverage includes isolated crop/straighten, identity/extremes, 16 validation/success cases, all seven manual transform controls, Auto/Guided Upright, combined crop/straighten, repeatability, cancellation, and zero fallback GPU transfers. Existing active crop tests still assert one compute submission/upload/readback.
- Successful results require matching dimensions/channel counts, finite pixels, and <=1e-4 absolute linear error. Error cases compare CPU and GPU error details.
- Existing vendor LibRaw C++ compiler warnings remain in the log; the requested Clippy gate passed.
- git diff --check passed. Only allowed pipeline-gpu and M2-09c evidence paths changed. No commits were made.
