# M1-05 verification

Implemented in /Users/rutmehta/Developer/tessera/.worktrees/M1-05 on wp/M1-05.
Only allowed paths changed. engine-api and its contracts are unchanged. The
pre-existing brief.md change was preserved. Historical BLOCKERS.md notes are
marked resolved, with their original contents retained below that notice.

## Required verification, actually executed

CARGO_TARGET_DIR remained exported as
/Users/rutmehta/.cache/tessera-target/M1-05 for every Cargo command.
No worktree target/ directory was created.

    cargo test -p pipeline-cpu -p raw-decode -p libraw-ffi --release && cargo clippy -p pipeline-cpu -p raw-decode -p libraw-ffi --all-targets -- -D warnings && cargo fmt --check

Exit code 0. 29 tests passed, none failed. Full output is verification.log.
LibRaw's existing vendored C++ build emits warnings; Rust clippy with -D warnings
passed. No vendor source or build flags were changed to hide those warnings.
`git diff --check` also passed.

## Evidence beyond compilation

- LibRaw test seeds deliberately different cam_xyz and rgb_cam values, including
  the fourth row/column, and confirms lossless independent extraction.
- RAW fixture test compares RawMetadata fields to directly read LibRaw fields.
- MHC impulse tests verify all 25 taps for every Bayer phase against the paper.
- Bilinear and MHC preserve flat colour fields; X-Trans tests cross tile seams
  and image edges with the global six-pixel phase.
- Highlight clip/propagation are distinguished by a synthetic clipped patch.
- CameraProfile followed by CAT16 neutralizes an as-shot grey patch.
- +1 EV exactly doubles linear values before display; neutral tone is bit-exact.
- Every extreme combination of contrast/highlights/shadows/whites/blacks remains
  finite and monotone over the 0..4 reconstructed range at +10 EV.
- Positive tint corrects toward magenta. Invalid/unsupported settings error.
- Sigmoid is black anchored, .18 pivoted and monotone over contrast/skew ranges.
- RGB rendering handles partial edge tiles and is deterministic.
- The missing-fixture skip branch was executed with PIPELINE_RAW_FIXTURES pointing
  to a nonexistent path, and printed the expected skip notice.

## Goldens

Rendered every one of the five RAW fixtures at 1/8 scale, using default settings.
Created PNGs once, visually inspected all five, and rerendered against existing
files. Every comparison had maximum absolute error 0/255 (threshold 2/255).
Goldens total 2,253,653 bytes. PNGs are tagged sRGB.

| File | Dimensions | SHA-256 |
|---|---|---|
| canon-cr3.png | 500x500 | 5b69d6ee4131d1bcc0d71b0512eccd6a8398120f44f16866af67ca518347b169 |
| fuji-raf.png | 612x408 | 715ddf7f315d3b1fa21288ee33f75fe77984691f78c0ee6a7d3be1a3a6e31afd |
| nikon-nef.png | 923x616 | d5afe88653e0800d5d851d1c76a4aa8c255df4138f473ca46616f631ff8fb846 |
| sample.png | 652x434 | b0de88ad190c3d99277a255b9ae6a185f7d846b020363dfe9e6be4d7bc05b53b |
| sony-arw.png | 615x410 | 1d4a1e60d7afb57a1cebc8b24b87e41a989c778162244e22b099aef21fe800c5 |

## Independent review

A read-only independent reviewer inspected tracked changes, all new source and
tests, OPERATORS.md, and relevant contracts. Verdict: passed, no material logic
or security findings. Nonblocking suggestions: nonconstant Bayer seam oracle,
non-finite standalone sigmoid validation, and more invalid-metadata tests.
Static scan found no hardcoded credentials, shell invocation or unsafe
serialization patterns in changed Rust source.

## Scope / limitations

See crates/pipeline-cpu/OPERATORS.md for exact formulas, source-paper attribution,
API semantics, supported controls and explicit M1 no-op stages. Auto maps to MHC
without adding a contract enum. X-Trans remains a documented mean interpolator.
Display kernel contrast/skew are exposed independently, while render uses fixed
defaults because the current recipe has no such fields. This implementation does
not claim Adobe/darktable bit matching, full colour-profile support, or recovery
of detail lost to sensor clipping.

Kanban orientation returned no task ID/environment assignment, so this run has
no associated card lifecycle transition.

RESULT: PASS
