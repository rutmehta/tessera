# M2-09b handoff

Implemented inside the M2-09b worktree only. No commits. Engine API unchanged.

## Delivered

- `lens::LensDataPack`: explicit on-demand HTTPS download, pinned upstream Lensfun v0.3.4 commit and SHA-256, atomic cache under `app-dir/lens`, integrity revalidation, returned version/license/attribution metadata, bounded archive/XML parsing, unsafe path/link rejection, existing profile loader integration. Synthetic tests cover corruption, malformed/unsafe archives, XML bounds and cache reuse. Licensing and attribution documented in `docs/13-licensing.md`.
- Ordered WarpRectilinear (shared/per-channel), FixVignetteRadial and GainMap (opcode 9) execution. List1 runs on full-sensor CFA before demosaic, List2 on linear camera RGB, List3 after camera profile/white balance. CFA resampling preserves phase. Embedded selection wins over database/image calibration, without applying corrections twice. Existing raw-decode payload preservation needed no changes.
- Independent `ManualCaSettings.red_cyan` and `.blue_yellow` in `LensContext`, plus numeric CRS mapping via `ManualCaSettings::from_crs`. Manual CA and purple/green hue-selective defringe run in the lateral pass before channel mixing. Fixed full-circle hue intervals. Resolved rendering preserves manual adjustments when embedded calibration replaces an injected profile.
- `GpuStageOp::demosaic_ca_batch`: demosaic, neighbour gather and sensor-frame CA share one transaction and final readback. Existing shader math unchanged. CPU parity gates cover seams, crops, band origins, Bayer phases, X-Trans, missing dependencies and cancellation; transfer counters assert one submission/readback. Metadata-bearing staged opcodes conservatively fall back to CPU in the bridge and managed export entry points.

## Verification performed by parent

With `CARGO_TARGET_DIR=/Users/rutmehta/.cache/tessera-target/M2-09b`:

```
cargo test -p lens -p pipeline-cpu -p pipeline-gpu -p raw-decode --release && cargo clippy -p lens -p pipeline-cpu -p pipeline-gpu -p raw-decode --all-targets -- -D warnings && cargo fmt --check
```

Exit 0. Aggregated test reports: 286 passed, 0 failed, 11 ignored. GPU parity tests instantiate the actual adapter, not a silent skip. Existing vendored LibRaw C++ warnings remain; Rust clippy passes with warnings denied. `git diff --check` passes. Full combined log: `verification.log`.

Explicit live network test also passed:
`cargo test -p lens --release --test data_pack downloads_pinned_upstream_pack -- --ignored --nocapture`.
It downloaded and verified the pinned archive, loaded 470 profiles, reported 835 unsupported/uncalibrated profiles, and verified offline cache reuse. See `live-pack.log`.

Real fixture gate inspected five RAW files, zero with opcode payloads. It will render future opcode-bearing fixtures when present. See `fixture-coverage.log`. The staged synthetic tests cover application order, gain interpolation/area/pitch, channel mixing, CFA phase isolation, priority and malformed payloads.

## Required integration outside allowed paths

- Persist/hash two future recipe fields: `/settings/lens/manual_ca_red_cyan` and `/settings/lens/manual_ca_blue_yellow`, finite float, zero identity, clamp -100..100. Map `crs:ChromaticAberrationR/B` (currently Legacy in the sidecar table) to them, then supply `LensContext.manual_ca`. The local numeric mapper already exists; no sidecar or engine-api schema was modified.
- Existing defringe fields and CRS mappings already exist; no additional schema fields are needed. CPU hue endpoints are degrees, whereas CRS endpoints are scaled by 3.6 in the existing mapper.
- Generic image-core `StageOp` has no CA operation. Nonresident graph callers must explicitly adopt `demosaic_ca_batch`; out-of-scope graph wiring was not changed. Existing resident paths already avoid intermediate readback.
- Nonzero independent manual CA, defringe and staged embedded opcodes require the CPU reference fallback (`ResolvedLens::plan` returns None). Low-level resident calls without metadata must preserve that capability check.
- The pack exposes version and attribution for the application to display; no UI paths were in scope. Unsupported profiles are reported, not approximated. No auto-update check was added.

Further stage and CRS details: `crates/pipeline-cpu/STAGED_OPCODES_M2.md`; GPU API details: `crates/pipeline-gpu/OPERATORS.md`.

RESULT: PASS
