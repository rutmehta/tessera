# Recovered M5-30/M5-31 integration repair — 2026-09-27

Base: recovered round-two branch integrated with main at `953204e` (main base
`c0d4535`). Parent reviewed the narrow implementation changes. This note records
correctness repair and validation; it does not accept the pending timing target.

## Changes and observed failures

- Explicit font snapshots now reach style-source, ordinary smart-child and
  filtered-child renderers, including the CPU filter fallback. Replacing fonts
  clears dependent style, stack and child caches. An explicitly empty database
  remains empty, rather than triggering system font discovery. The helper in
  `render/live.rs` is an intentional M5-30 compatibility change authorized by the
  parent; the original M5-31 brief allowlist predates this integration.
- Unfiltered smart children containing styles now use the requested child level
  and normal viewport rebasing. The obsolete styles-only filter route rendered
  L0 and downsampled it, contrary to CPU level-local style semantics. Enabled
  filters retain their native-resolution route; disabled-only filters do not.
- Style-source generated text/shape pixels and effective vector masks retain F32
  precision in the existing float page pool. Native raster uploads and their
  exact integer mip arithmetic remain unchanged. The raster fast path is
  disabled for these generated float sources; both interpreter and specialized
  composition are exercised.
- Before source conversion or effect-plane generation, style preflight reserves
  source bytes, the exact enabled plane expansion, per-plane metadata and earlier
  pending layers against the auxiliary binding limit. Inner/Outer bevel emit two
  planes; Emboss/Pillow emit four. Disabled effects emit none; enabled zero-size
  or zero-opacity effects retain their existing allocation/semantic behavior.
  This bounds unusable aggregate plane allocations, not total device memory.

The tests were written and observed failing before the corresponding fixes:

- All four initial live tests failed: unavailable explicit Noto font, empty font
  database falling back to the system, stale planes after font replacement, and
  true U8 Shape parity error `0.0018455088` (tolerance `1e-4`).
- After font forwarding, the smart-child L1 test exposed error `0.09038784` from
  the obsolete L0 route. Direct and isolated-group cases already passed.
- A native raster with a vector/raster mask exposed error `0.0022019595` before
  generated mask precision was corrected.
- A 16×16 image with 64 Emboss bevels and a 1 MiB binding limit was rejected only
  after creating the effect pipeline/planes. The fixed test verifies rejection
  before pipeline creation, zero evaluations and no retained style buffers. No
  GiB-sized failure fixture was allocated.

## Validation

Commands use `CARGO_TARGET_DIR=/Volumes/betterSSD/tessera-cache/target/M5-31`.
The shared build/GPU slot was exclusive. No unrelated app or workspace test
suite was run.

- `cargo test -p compositor --release`: **323 passed, 0 failed, 13 ignored**,
  exit 0 across 59 result summaries (including zero doc tests).
- Focused styles unit/kernel/viewport run: **24 passed**, including actual plane
  counts for every enabled/disabled effect and bevel kind, zero-size/opacity
  effects, cumulative data/metadata boundaries and checked-size overflow.
- Focused live integration run: **5 passed**. Seven font placements cover direct
  text, isolated group, styled child, styles on a smart object, resident filter,
  CPU filter fallback and disabled filter. Checks include explicit empty fonts,
  font replacement/restoration and L0/L1/L2. True Shape tests cover U8/U16/F32;
  combined vector/native raster masks cross a tile boundary.
- `cargo clippy -p compositor --all-targets -- -D warnings`: **passed**, exit 0.
  An initial run rejected taking a mutable reference to `usize::MAX` in an
  overflow test; the test now uses a local variable with unchanged semantics.
  No production code changed after the complete release gate.
- `cargo fmt --check`: **passed**, exit 0 after that test-only correction.
- `git diff --check`: **passed**.

Local evidence (preserved on Machine A):

- `/tmp/tessera-m531-live-red.log`
- `/tmp/tessera-m531-preflight-red.log`
- `/tmp/tessera-m531-font-diagnose.log`
- `/tmp/tessera-m531-mask-red.log`
- `/tmp/tessera-m531-live-final.log`
- `/tmp/tessera-m531-focused-final.log`
- `/tmp/tessera-m531-full-release.log`
- `/tmp/tessera-m531-clippy-final.log`
- `/tmp/tessera-m531-fmt-final.log`

## Acceptance still outstanding

Machine B must run the strict ignored 20 MP, five-style, 1368×912 L1 timing
acceptance in multiple fresh processes on the M4 Max with no competing builds or
GPU jobs. Preserve all cold and dispatched-warm samples, exact SHA, hardware and
commands. CPU `<2 s` and resident `<100 ms` assertions remain unchanged. Historical
cold resident failures of 110.561–253.233 ms are not overwritten by these
correctness gates. The ignored timing tests were not run here. This repaired
branch is a provisional acceptance candidate, not READY and not approved for
main merge on correctness alone.
