# M5-21 execution handoff

## Delivered

- Apache-2.0 transform workspace member: premultiplied planar CPU affine/homography, Bézier mesh/presets/subdivision/Newton fallback, continuous quad perspective, deterministic local/global ARAP puppet mesh, both-axis forward-energy seam carving with protect/skin hook.
- Versioned serde TransformOp with level-aware inverse rendering, shared displacement output and automatic kernel selection.
- Compositor smart-filter stage, AddTransform/SetTransform history operations, native validation and PSD affine placement/embedded source roundtrip.
- Explicit resident RG32Float displacement texture stage with mandatory precise Metal compilation and CPU comparison tests.
- Formulas and limits in crates/transform/TRANSFORM.md and crates/compositor/src/resident/TRANSFORM.md.

## Verification performed by parent

Required command completed with exit code 0:

    cargo test -p transform -p compositor --release && cargo clippy -p transform -p compositor --all-targets -- -D warnings && cargo fmt --check

Real Apple M4 Metal parity: nearest/bilinear/bicubic exact in tested cases; worst observed Lanczos absolute error 4.7683716e-7.

36MP CPU bicubic final rerun (includes validation, allocation, rendering): 646.437 / 600.760 / 553.902 ms. Earlier delegate runs were 280–312 ms, but parent did not reproduce <400 ms. Concurrent rustc and sibling GPU workloads were observed, not interrupted.

36MP GPU bicubic median: 18.568 ms cached compute, 227.733 ms map-generation/upload/dispatch/wait. Lanczos median: 32.901 ms.

All modified/untracked source files verified within allowed paths. CARGO_TARGET_DIR stayed /Volumes/betterSSD/tessera-cache/target/M5-21. No local target directory or commits.

## Remaining acceptance gaps

- CPU <400 ms was not reproduced by parent; end-to-end geometry-change GPU latency exceeds 30 ms (cached bicubic compute meets it).
- Resident transform stage is explicit, not automatic document smart-filter routing; filter-stack rendering continues to use CPU.
- lens::Homography is reused, but public reusable RGBA kernels do not exist in lens at this revision. Sampling is implemented in transform and matched in WGSL; the requested shared cross-pipeline kernel extraction/reuse is not done.
- Puppet shape uses conservative grid triangulation, not contour-constrained triangulation. Native-only transform stacks reject PSD export with an explicit rasterization request. More detailed limitations are documented.

RESULT: FAIL CPU performance target not verified; kernel-sharing and automatic resident stack routing remain incomplete.

## Retry verification and lock correction (latest)

- Reproduced and fixed the position-lock bypass in `SetSmartFilters`, including
  transform removal, enable/blend changes and reordering. `SetSmartTransform`
  now observes the same position/all locks as AddTransform/SetTransform.
- Added `crates/compositor/tests/transform_locks.rs`. The regression failed on
  locked whole-stack replacement before the fix; both tests now pass. Rejected
  edits leave serialized state unchanged; color-only edits at unchanged
  transform indices still work under a position lock and can be undone.
- Ran the exact required gate after the fix: exit 0, 186 passed, 0 failed,
  9 ignored. Clippy and workspace fmt check passed. See `gate.log`.
- Final CPU 36 MP bicubic: 265.370 / 259.059 / 262.638 ms. The CPU target is now
  reproduced. Earlier retry samples were 367.351 / 331.614 / 590.980 ms.
- Final GPU bicubic median: 15.969 ms cached compute, 16.312 ms cached
  submit/wait, 104.551 ms including geometry-map preparation/upload/dispatch/wait.
  Lanczos-3 GPU median: 32.812 ms. See actual output in `benchmarks.log`.
- `CARGO_TARGET_DIR` remains `/Volumes/betterSSD/tessera-cache/target/M5-21`.
  No commits, changes outside allowed paths, or repository-local target directory.

### Scope blocker and remaining work

`crates/lens/API.md` explicitly says there is no image resampling in lens.
The existing composed map is in `crates/pipeline-cpu/src/lens_plan.rs` and its
private reconstruction kernels are in `geometry_effects.rs`, outside the allowed
edit paths. A shared extraction requires permission to edit lens/pipeline-cpu
(and their existing consumers), or an explicit waiver of shared-kernel reuse.
Do not claim the current independent transform sampler satisfies that requirement.

Automatic document smart-filter routing through the resident displacement stage
is still unimplemented. The explicit stage and CPU document path work and pass
their tests; this retry did not change that integration boundary. Grid-based
puppet silhouette approximation and native-only PSD stack export limits remain
documented in TRANSFORM.md.

RESULT: FAIL shared resampling-kernel reuse and automatic resident smart-filter routing remain incomplete; required test gate and cached bicubic CPU/GPU benchmark targets pass.

## Subsequent verification

Re-ran the exact required test/clippy/fmt command in the M5-21 worktree. It
returned exit code 0. `git diff --check` also returned 0. Verified all modified
and untracked paths against the allowlist, no repository-local target directory,
and the unchanged external CARGO_TARGET_DIR. No source changes or commits were
made during this verification; benchmark figures above are prior-run evidence,
not new measurements.

Independently confirmed the scope conflict: `crates/lens/API.md:33` explicitly
excludes image resampling, and the existing Lanczos function is private in
`crates/pipeline-cpu/src/geometry_effects.rs:138`. A proper shared extraction
requires expanding the allowed paths, rather than copying private source through
an include or claiming independent kernels constitute reuse. The resident API
still documents explicit invocation rather than automatic stack routing.

No Kanban task ID was supplied in this execution environment; `kanban_show()`
returned `task_id is required`, so no board lifecycle transition was possible.

RESULT: FAIL shared-kernel reuse is blocked by the allowed-path scope; automatic resident smart-filter routing remains incomplete despite a passing required gate.
