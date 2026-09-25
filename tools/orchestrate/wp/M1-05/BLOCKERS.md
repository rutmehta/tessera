# Historical M1-05 blockers (resolved by the expanded brief)

The current implementation is no longer blocked. The expanded allowed paths
include libraw-ffi, so cam_xyz and rgb_cam now pass through as independent raw
fields. MHC was independently implemented from the 2004 paper, replacing the
previously requested GPL RCD port. See crates/pipeline-cpu/OPERATORS.md and this
directory's VERIFICATION.md for the current implementation and verification.
The notes below describe earlier runs and are retained as history only.

No production code or engine-api contracts were changed. The existing pipeline-cpu crate remains a placeholder. Passing its current checks is not acceptance of M1-05.

## Required matrix is unavailable through the allowed boundary

The task requires the LibRaw XYZ-to-camera `cam_xyz` matrix, inverted and normalized for CameraProfile before CAT16 WhiteBalance.

- `crates/raw-decode/src/lib.rs:33-35` documents `RawMetadata::camera_to_xyz` as a conversion for **white-balanced** camera RGB.
- `crates/libraw-ffi/src/sensor.rs:70-83` constructs that conversion from `rgb_cam`, not `cam_xyz`.
- `crates/libraw-ffi/src/lib.rs:22-25` keeps the LibRaw handle private. Its public metadata/image types do not expose `cam_xyz`.

Adding a field only to RawMetadata cannot supply the missing original matrix. Inverting the existing conversion and calling it cam_xyz would misrepresent its provenance and normalization. Reopening LibRaw through a duplicate unsafe FFI in raw-decode would bypass the established decoder boundary rather than fix it.

Unblock: allow changes to `crates/libraw-ffi/**` (or have its owner expose the original matrix), then forward that field through RawMetadata. No engine-api change has been identified as necessary.

## RCD port licensing decision

The requested public RCD source is https://github.com/LuisSR/RCD-Demosaicing . Its LICENSE, retrieved directly from https://raw.githubusercontent.com/LuisSR/RCD-Demosaicing/master/LICENSE , is GNU GPL version 3.

`docs/13-licensing.md:10-16` expressly bans GPL-2/3 dependencies, and the current pipeline-cpu manifest declares MIT. Do not port that implementation under an incompatible license or label a simpler interpolator as RCD.

Unblock: identify an appropriately licensed implementation or authorize an independently implemented algorithm from a reviewed mathematical specification rather than a source-code port. A licensing-policy exception requires explicit review.

## Verification actually executed

With CARGO_TARGET_DIR=/Users/rutmehta/.cache/tessera-target/M1-05:

    cargo test -p pipeline-cpu --release && cargo clippy -p pipeline-cpu --all-targets -- -D warnings && cargo fmt --check

Exit status: 0. Only the existing `crate_is_available` placeholder test ran (1 pass). No operators, RAW render tests, or goldens were implemented or claimed as tested.

The Kanban orientation call returned `task_id is required (or set HERMES_KANBAN_TASK in the env)`, so no card lifecycle transition was available for this run.

## Retry verification

Re-read engine-api/CONTRACTS.md first, then independently checked the current
raw-decode metadata and libraw-ffi sensor implementation. The boundary still
exposes only the rgb_cam-derived matrix, not cam_xyz. Re-fetched the upstream
RCD LICENSE and confirmed GPL-3 against the repository's explicit ban in
docs/13-licensing.md. Neither blocker has been resolved by this retry's scope.

Re-ran the exact requested release-test, clippy, and fmt command with the
external CARGO_TARGET_DIR exported. It exited 0, but again exercised only the
single placeholder test. This is not a passing implementation. No production
code, contracts, or golden images were changed. Before another implementation
retry, expose cam_xyz through libraw-ffi (or expand the allowed paths) and
resolve whether an independent RCD implementation is acceptable instead of
the requested GPL source port.

RESULT: FAIL required cam_xyz exposure needs an out-of-scope libraw-ffi change; RCD source port conflicts with repository licensing policy.
