# B5-34 — Camera Raw detail omission across the stack

Branch: `wp/B5-34`, based on `origin/main` at `68264c74`. Local commits only; no push.

## Problem and root cause

At zoomed-out canvas levels, lower Camera Raw smart filters still evaluated Sharpening, Noise Reduction, Texture and Clarity with level-0 pixel radii. B5-18b removed those effects only from the `StackEdit` being previewed. Saved-stack bakes evaluated the original nodes, and the bake cache allowed a finer render (including level-0 refinements) to satisfy a coarser view. The sheet also inspected only the edited draft when deciding whether to show the detail note.

## Fix

- Apply the existing detail-removal operation to every Camera Raw node in presentation stacks, both saved bakes and edit-preview worker jobs. Preserve node enabled state, blending, opacity, masks and stored recipes.
- Carry the resolved canvas level from the `document/render.rs` presentation path into bake jobs, separately from their evaluation level. Stacks containing an enabled full-resolution adapter retain detail at every zoom, matching the edit preview and its level-0 note policy (see review correction below).
- Give enabled Camera Raw stacks a level-specific bake key, preventing a finer bake or level-0 refinement from leaking into a later zoomed-out render. Other stacks retain their existing finer-level cache reuse.
- The Camera Raw sheet includes other enabled, nonzero-opacity Camera Raw stages when deciding whether to show `Detail effects preview at 100 %`. It excludes the saved row being replaced, honors zero Amount, and continues to use the engine-reported preview level.
- Level 0, Apply, export and the 1:1 detail pane evaluate the original settings. No public FFI API change; regenerated bindings are unchanged.

## Tests and before/after numbers

`crates/tessera-ffi/tests/document_camera_raw_preview.rs::stacked_camera_raw_detail_follows_canvas_level` uses a 256 × 192 image with detail at several scales and two active Camera Raw filters. It exercises levels `2 → 0 → 1 → 2 → 0`, both re-edit positions, a 128 × 96 visible-region comparison against a committed smart stack rendered through the independent exact export path, the 1:1 pane and export while zoomed out, and unchanged saved JSON.

Maximum absolute RGBA differences are normalized to 0…1:

| Comparison | Before | After |
|---|---:|---:|
| L2 saved stack versus both stages with detail zeroed | 0.5058824 (129/255), FAIL | 0, exact |
| L1 saved stack versus both stages with detail zeroed | not separately recorded | 0, exact |
| Re-edit either stage versus saved stack, L0/L1/L2 | not separately recorded | 0, exact |
| L0 versus smart-stack Apply, visible region | not separately recorded | 0 (limit 2.5/255) |
| L0 versus zero-detail stack | not separately recorded | 0.6588235, proving detail remains active |
| 1:1 pane while at L2 versus Apply | not separately recorded | 0 (limit 2.5/255) |
| Export while at L2 versus Apply | not separately recorded | 0 (limit 2.5/255) |
| Swift note with neutral upper stage and sharpening below, L1/L2 | absent; 2 assertion failures | passed |

`DocumentCameraRawTests.testDetailNoteIncludesOtherCameraRawStages` drives the real sheet model and engine backend. It checks the note at levels 1 and 2, no note at level 0, resetting the only detail-bearing stage, and a disabled lower stage.

Red-first evidence: Rust failed with `all saved Camera Raw stages omit detail at L2`, actual `0.5058824`, expected `0.0`. Swift ran one test with **2 failures (0 unexpected)**, both for the missing lower-stage note. The failing-test commit precedes all production changes.

Fixture corrections: opening the same path returns the same document session, so the reference images use distinct paths. The original 100% reference used two destructive U8 Applies; those clamp/quantize intermediates, unlike a smart stack. The final reference applies both smart filters and uses exact export, retaining the original 2.5/255 tolerance. No production level-0 behavior was changed to accommodate that reference.

## Gates

This package ran its build commands serially, with `PATH="$HOME/.cargo/bin:$PATH"` and `CARGO_TARGET_DIR=$HOME/.cache/tessera-target/B5-34`.

- `cargo test --locked --release -p tessera-ffi`: **536 passed, 0 failed, 28 ignored** (49 result summaries including doc tests).
- Focused stacked Camera Raw test: **1 passed, 0 failed**; all comparisons above logged with `--nocapture`.
- `cargo clippy --locked --all-targets -p tessera-ffi -- -D warnings`: exit 0, default dev profile.
- `cargo fmt --all -- --check`: exit 0, no output.
- `cd apps/mac && ./build-ffi.sh`: exit 0; archive reports arm64.
- `tools/orchestrate/swift-gate.sh`:

```text
Build complete! (68.77s)
	 Executed 862 tests, with 3 tests skipped and 0 failures (0 unexpected) in 482.668 (482.811) seconds
✔ Test run with 5 tests in 2 suites passed after 0.046 seconds.
SWIFT GATE OK
```

The builds emitted existing LibRaw C compiler warnings and a linker warning about `blake3_neon.o` targeting macOS 26.2 while linking for 15.0. The red Swift build also emitted existing Sendable/weak-mutability warnings. Rust Clippy passed with warnings denied.

## Delivery and limits

- `7fe4e061` — `test(B5-34): cover stacked Camera Raw detail omission and note`
- `41854754` — `fix(B5-34): omit Camera Raw detail across zoomed-out stacks`
- This handoff is the following `docs(B5-34):` commit.

No changes to `board.json` or `Cargo.lock`. No GUI app launched and no screen captured. Verification is automated and numerical; no on-screen review was performed. The existing sheet contract still follows the last submitted preview's engine level; this package does not add resubmission on zoom alone.

## Continuation verification

The continuation retained the original failing-test commit and production edits. Completed gate logs from the interrupted run were inspected: all are newer than the last source/test edit (00:43:16 on 2026-10-01); Rust finished at 00:52:46, Clippy and formatting at 00:53:51, FFI at 00:56:37, and the Swift gate at 01:09:44. No source was changed by the continuation. A redundant focused-test rebuild was stopped after these completed results were recovered; it is not counted as verification. `git diff --check` passed before committing.

Selected raw red/green and gate output is retained in `EVIDENCE.txt`.

## Machine A review correction: full-resolution stacks

Review found that a stack containing Camera Raw plus an enabled full-resolution
adapter (Liquify, content-aware or neural) had conflicting presentation policies:
the edit preview forced level 0 and retained detail, while the saved bake stripped
detail using the zoomed-out canvas level. The sheet correctly showed no omission
note for its level-0 preview.

The bake now uses `if full_resolution(&nodes) { 0 } else { b.canvas_level }`
for detail stripping. The `|camera-raw-level` cache-key suffix is added only when
the stack does not require full resolution. Such full-resolution stacks reuse
their unstripped bake across zoom levels; enabling/disabling stages remains part
of the base key. The sheet's existing engine-level note policy needs no
production change.

Additional regressions:

- Camera Raw plus identity Liquify at L2: saved canvas and edit preview must be
  exactly equal; comparison with explicitly stripped settings proves the edit
  retains detail. The engine reports preview level 0.
- Actual Swift sheet model with Camera Raw plus Liquify at L2: detail-bearing
  draft, preview level 0, and no omission note.
- Camera Raw at 35% Amount: L1 canvas and edit equal a zero-detail reference;
  L0 remains visibly different.
- Stored smart-filter mask at L1: stripping preserves mask, blend, opacity,
  enabled state, Amount and exposure, zeros all global detail effects, and does
  not mutate the saved recipe. This is a node-level test: the public Camera Raw
  smart-filter Apply API rejects active selections.
- Disable/re-enable through L0/L1/L2: rendered comparisons plus direct cache-key
  assertions cover enabled-state and effective-level changes.

RED before the production fix:

```text
camera_raw_canvas_key_tracks_effective_level_and_enabled_state ... FAILED
full-resolution Camera Raw stacks reuse the unstripped bake across zoom levels
B5-34 full-resolution L2 canvas vs edit: 0.16862746
assertion `left == right` failed: full-resolution L2 canvas and edit must both keep detail
  left: 0.16862746
 right: 0.0
```

The corrected RED fixtures passed the Amount, mask and toggle tests, with only
the intended cache-key and full-resolution pixel failures remaining.

Performance and existing limitation: each zoom-level change now re-runs the
whole bake for level-sensitive Camera Raw stacks (one bake per layer), rather
than reusing a finer bake. Full-resolution stacks are exempt after this
correction because their detail policy stays at level 0. Sharpness, clarity and
texture inside Camera Raw local masks are still not stripped; this is
pre-existing behavior and is separate from the stored smart-filter mask covered
above.

### Fresh review-correction gates

- Full release Rust tests: **541 passed, 0 failed, 28 ignored** (49 summaries).
- Clippy, all targets with warnings denied: exit 0.
- Formatting check: exit 0.
- Fresh FFI regeneration: exit 0, arm64 archive; generated bindings unchanged.
- The first fresh Swift gate had **863 tests, 3 skipped, 1 failure** in
  `ShellLayoutTests.testDocumentInspectorEveryTabAndHistoryStateAtEverySize`.
  The new Camera Raw note test passed. A focused rerun reproduced four stale
  or missing `historyBody` geometry assertions.
- The same layout test passed with `CFFIXED_USER_HOME` set to a temporary
  preferences home. Concurrent Swift test runners were present, and the
  harness writes `InspectorPanel.History` to `UserDefaults.standard`; shared
  preferences are a plausible cause, not a proven diagnosis. No layout code,
  test assertions, or gate script was changed. The full gate was rerun with
  that preferences isolation; PATH and CARGO_TARGET_DIR remained as required.

Local review-correction commits: `50ca23a7` (tests), `cd2d1a51` (fix), followed
by this documentation/evidence commit. All are on top of `2b6b1c80`, with no
rebase or push.

The isolated full gate also failed: **863 tests, 3 skipped, 1 failure**, now in
`ShellLayoutTests.testShellContainedAtEverySizeStateAndAppearance` (too few
native controls in document windows). The original inspector test and the
Camera Raw note regression passed in that run. Preferences isolation therefore
did not resolve the full-gate problem.

A third fresh gate, back in the standard environment after the then-active
XCTest processes exited, also reports failures outside Camera Raw:
`DocumentHistoryKeyboardTraversalTests.testExternalPreferenceWriteUpdatesHostedReadout`
(hosted height control absent),
`MasksPanelLayoutTests.testPopulatedInspectorKeepsComponentActionsReadableAtMinimumWidth`
(image source absent), and
`ShellLayoutTests.testDocumentInspectorEveryTabAndHistoryStateAtEverySize`
(stale/overlapping history geometry). The Camera Raw note regression passed
again. These failures do not establish a production regression from B5-34,
but no baseline comparison proves them pre-existing either.

**Delivery gate remains unmet: no fresh run printed SWIFT GATE OK.** Do not
treat the older successful gate earlier in this handoff as verification of this
review correction. No unrelated layout or test code was modified or disabled.

The third retry stopped producing log output for nearly six minutes after its
failures (last write 02:25:24, checked 02:31:12 local time). Its exact worktree
XCTest process was terminated; the gate then returned exit 1 and listed the
three failures above. This retry has no complete suite count. The first two
failed full runs completed normally. No other worktree process was stopped.
All builds were serial within this worktree, and no GUI app was launched.
`Cargo.lock` and `board.json` are unchanged.
