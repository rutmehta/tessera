# M2-47 retry verification and acceptance status

## Changes in this retry

- Fixed `PhotoOutput.source_ids` losing all but the first merge input when `create_stack=false`. Publication now carries source provenance independently from the stack flag. Both merge and enhance use the shared publication contract.
- Added `unstacked_merge_preserves_all_source_ids`. It failed with one source instead of two before the fix (`provenance-red.log`), then passed in the complete gate.
- Fixed the existing print-profile test's timestamp flake without changing export production code: compare the rendered ICC bytes with the exact profile written to disk, not a newly generated profile. The first captured gate failed at `tests/export.rs:505` because ICC creation timestamps differed by one second (`retry-gate.log`).

## Executed verification

Ran the exact requested chain in this worktree, with `CARGO_TARGET_DIR=/Volumes/betterSSD/tessera-cache/target/M2-47` preserved:

```
cargo test -p tessera-ffi -p merge -p ml-enhance --release && cargo clippy -p tessera-ffi -p merge -p ml-enhance --all-targets -- -D warnings && cargo fmt --check && (cd apps/mac && ./build-ffi.sh && swift build)
```

Final run `proc_743fe7dfa22e` exited 0. Complete output: `final-gate.log`, ending `GATE_EXIT=0`. Rust test summaries aggregate to 214 passed, 0 failed, 9 ignored. Model-dependent tests retain their conditional cache-based skips; those totals do not imply production weights were exercised. Swift completed in 95.82 seconds. Generated binding read-back confirmed photoMerge, mergePreview, enhance, photoStack, PhotoJob and EnhanceOptions. The macOS 26.5 versus 15.0 blake3 object linker warning remains.

The initial foreground gate call hit the tool's 420-second timeout without a returned result. It was not counted as success. The captured retry and final gate provide the verifiable outcomes.

Path allow-list audit found no out-of-scope changed/untracked paths. Branch remained `wp/M2-47`. No commits or pushes. No Kanban task ID was injected, so `kanban_show()` returned a missing-task-ID error; no board state was changed.

## Acceptance is still FAIL

This retry does not claim the entire work package is implemented. Existing limitations in `HANDOFF.md` remain, notably:

- Boundary Warp 1..100 is rejected rather than implemented. The merge core documents crop/nearest-fill as placeholders, not a boundary mesh or thin-plate spline.
- Raw Details is rejected. The required learned demosaic model and runtime contract are absent from the supplied enhancement library/model manifest. Classical demosaic has not been relabeled as learned Raw Details.
- Fill Edges is nearest-covered extension, not the content-aware operation specified in docs/01.
- Native float LinearRaw outputs index successfully, but the general image-core reader still opens non-RGB inputs through `RawSource::decode_cfa` (`crates/image-core/src/source.rs:52-60`). Supporting these outputs in the normal grid/develop path requires changes outside this task's allowed paths.
- Auto projection chooses Perspective rather than geometry-based selection. Nonidentity orientation and nonzero enhancement of HDR/out-of-sRGB-gamut inputs remain unsupported.

To finish acceptance, implement and verify the missing panorama operations, supply a vetted learned-demosaic model contract, and authorize the native LinearRaw reader/render integration. Green compilation alone does not close these requirements.
