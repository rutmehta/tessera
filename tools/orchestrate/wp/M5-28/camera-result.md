# M5-28 Camera Raw implementation

Implemented in this worktree, no commits. Changes are confined to the requested image-core/filter paths and `camera-*` artifacts. The other worker's compositor changes were not edited. No manifests or dependencies changed. The pre-existing `RgbSource::from_raster` implementation and tests are preserved.

## Implementation

- Camera Raw CPU evaluation now calls `image_core::Renderer::render_rgb_linear` using a validated in-memory RGB source, with no file round trip.
- Renderer owns a persistent exact-f32 stage LRU: Lens analysis/alignment/defringe; WB/profile and manual vignette gains; Detail; Tone/presence/curves; Color; Locals; Effects; composed Geometry. It never uses the legacy f16 preview cache. It uses image-core StageOp for creative operators and the public resolved scalar entry point with neutral creative controls to access private optics operations. The neutral wrapper incurs identity-pass overhead; it does not rerun upstream creative adjustments with their active settings.
- Lens resolution belongs to the original source and lens settings. Automatic CA precedes WB, gains follow WB, and the common warp follows Effects. Nonneutral manual optics and actual automatically estimated CA/vignette corrections match the scalar reference exactly in the regression fixtures.
- Source identity/revision and chained stage settings control invalidation. WB edits retain lens analysis; tone edits retain detail; colour edits retain tone; crop edits invalidate crop-anchored effects. The adapter hashes exact input bits, every tile revision and colour/level/canvas interpretation because FilterContext has no layer ID. Same-revision changed pixels cannot reuse stale output. Image-core callers must provide an immutable image ID and revision.
- A regression caught raw u128 IDs collapsing to JSON null in the canonical hash helper. Keys now use ImageId's hex serialization, with a large-ID regression test.
- Cache lookups start with the latest retained stage, so an evicted ancestor is not rebuilt when its descendant remains resident. The adapter budget is 256 MiB of pixel payload, at most 64 checkpoints. RendererConfig controls the separate image-core RGB cache budget. Oversize frames are evaluated without retaining their checkpoints. Retained payload is bounded; full-frame working scratch and caller-owned outputs are additional. Calls sharing this cache serialize. Edits can recompute evicted upstream checkpoints under budget pressure.
- Profile handling, signed/HDR f32 samples, exact warm/cold output, alpha, amount, native input-level semantics and geometry canvas padding are covered by integration tests.

## Resident scope

Used the request's permitted documentation alternative for remaining GPU controls. `crates/filters/README.md` now records each exclusion and distinguishes valid RGB algorithms lacking a GPU implementation from unavailable metadata/depth/host bindings and shared engine gaps. Automatic lens/CA estimation, defringe and Upright remain genuine resident engineering gaps; they are not described as CFA-only limitations. Orientation/constrain-crop and softness remain shared implementation gaps. Embedded/database profiles and depth-dependent effects require inputs this adapter does not expose. Existing guards remain intact; no hidden readback or CPU fallback is presented as GPU completion.

## Verification

Every cargo command used:

    CARGO_TARGET_DIR=/Volumes/betterSSD/tessera-cache/target/M5-28

Test-first failure evidence is retained in `camera-red.log`, `camera-wb-red.log`, `camera-budget-red.log`, `camera-id-red.log`, and `camera-boundary-red.log`.

| Command | Result | Evidence |
| --- | --- | --- |
| `cargo test -p image-core --release --no-default-features --test rgb_memo --test rgb --test render` | Passed: render 10, RGB 2, then-current memo 6 | `camera-core-tests.log` |
| `cargo test -p image-core --release --no-default-features --test rgb_memo` | Passed: final memo suite 7 | `camera-memo-final.log` |
| `cargo test -p image-core --no-default-features --lib rgb::tests` | Passed: 10 | `camera-constructor-tests.log` |
| `cargo test -p filters --release --test camera_raw --test camera_raw_gpu` | CPU 7 passed; GPU capability 2 passed; GPU execution 4 failed at adapter creation; benchmark 1 ignored | `camera-targeted.log` |
| `cargo clippy -p image-core -p filters --all-targets -- -D warnings` | Passed, exit 0 | `camera-clippy.log` |
| Scoped `rustfmt --edition 2024 --check` on all changed Rust files; `git diff --check` | Passed, exit 0 | Executed after final edits |

38 distinct CPU/capability tests passed. Existing vendor LibRaw C++ deprecation warnings were emitted during builds; Rust clippy passed with warnings denied.

The four GPU execution failures all report `Metal adapter: No suitable graphics adapter found`. No GPU numerical result was produced, and no test was changed to skip missing hardware. The ignored 24MP GPU benchmark was not run. GPU runtime parity needs a host exposing a Metal adapter; it is not verified by this sandbox run.

Result: CPU memoized Develop integration and the requested per-control resident limitation inventory are implemented. Full resident Develop remains explicitly unsupported for the documented controls; hardware-dependent verification is unavailable in this environment.
