# M2-47 current verification: gate PASS, acceptance FAIL

This run inspected the existing implementation and its documented gaps, preserved it, regenerated Swift/C bindings, and executed the entire requested gate itself. No missing feature was implemented in this run. No commits or pushes were made.

## Executed gate

Working directory: `/Users/rutmehta/Developer/tessera/.worktrees/M2-47`

`CARGO_TARGET_DIR=/Volumes/betterSSD/tessera-cache/target/M2-47` was exported for the entire chain:

```
cargo test -p tessera-ffi -p merge -p ml-enhance --release && cargo clippy -p tessera-ffi -p merge -p ml-enhance --all-targets -- -D warnings && cargo fmt --check && (cd apps/mac && ./build-ffi.sh && swift build)
```

Process `proc_91d81bed0620` exited 0. Evidence: `current-gate.log`, ending `GATE_EXIT=0`. Parsed Rust summaries: 214 passed, 0 failed, 9 ignored. Conditional model tests may return early without cached weights; these totals are not proof that production inference ran. Swift build completed in 26.25 seconds. The linker still warns that a blake3 object targets macOS 26.5 while the application links for 15.0.

Read-back of regenerated Swift confirmed `photoMerge`, `mergePreview`, `enhance`, `photoStack`, `PhotoJob`, `PhotoJobListener`, `EnhanceOptions`, and `MergeOptions`. The branch remains `wp/M2-47`. The changed/untracked path audit found no paths outside the supplied allow-list. `git diff --check` reports trailing whitespace in generated bindings; this is separate from the requested gate, which passed.

## Unmet acceptance requirements

- Boundary Warp 1..100 is rejected at `crates/tessera-ffi/src/merge.rs:84`. No boundary mesh implementation exists in `crates/merge/src/pano.rs`; cropping is not a substitute.
- Fill Edges calls nearest-covered extension at `crates/merge/src/pano.rs:248`, not the content-aware fill required by `docs/01-lightroom-classic-spec.md:350`.
- Raw Details is rejected at `crates/tessera-ffi/src/enhance.rs:19`. The supplied enhancement library explicitly requires a separate trained model (`crates/ml-enhance/README.md:138`), and the model registry has no learned-demosaic entry. The CFA denoiser produces CFA planes, not learned-demosaiced RGB. Ordinary demosaic cannot honestly be relabeled Raw Details.
- Generated native LinearRaw DNGs are indexed, but the normal grid/develop source still invokes `RawSource::decode_cfa` at `crates/image-core/src/source.rs:57-60`. The FFI's private linear-DNG decode does not integrate that renderer. This reader is outside the allow-list.
- Auto projection unconditionally chooses Perspective. Nonidentity orientation and nonzero enhancement of HDR/out-of-sRGB-gamut input are still rejected, as already recorded in HANDOFF.md.

To unblock full completion, the task needs a supported learned-demosaic model contract and authorization/dependency work for native LinearRaw grid/develop ingestion. Boundary mesh warping and content-aware fill remain implementation work within the allowed merge crate. Rerunning the gate alone cannot resolve any of these gaps.

No Kanban task ID was injected: `kanban_show()` returned the missing-task-ID error. No board lifecycle transition was possible or claimed.

RESULT: FAIL requested features remain incomplete despite a green build/test gate.
